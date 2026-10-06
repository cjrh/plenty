mod support;

fn run_file(source: &str, bytes: Option<&[u8]>) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    if let Some(bytes) = bytes {
        std::fs::write(dir.path().join("sample.txt"), bytes).unwrap();
    }
    let executable = dir.path().join("program");
    support::compile_source_to_executable(source, &executable).unwrap();
    std::process::Command::new(executable)
        .current_dir(dir.path())
        .output()
        .unwrap()
}

#[test]
fn reads_utf8_and_universal_newlines() {
    for (bytes, expected) in [
        (&b""[..], ""),
        ("é\0\r\nx\ry\n".as_bytes(), "é\\0\\nx\\ny\\n"),
    ] {
        let out = run_file("print(read_text(\"sample.txt\"))", Some(bytes));
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("Result[str, IoError].Ok(\"{expected}\")\n")
        );
    }
}

#[test]
fn file_errors_do_not_abort() {
    for (bytes, expected) in [
        (None, "IoError.System("),
        (Some(&b"\xff"[..]), "DataError.InvalidUtf8"),
    ] {
        let out = run_file("print(read_text(\"sample.txt\"))", bytes);
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains(expected));
    }
    let out = run_file("print(read_text(\"sample.txt\\0suffix\"))", Some(b"secret"));
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("IoError.System(0)"));
    assert!(support::check_source("read_text(1)").is_err());
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reading_recovers_from_each_allocation_failure() {
    for budget in 0..=3 {
        let out = run_file(
            &format!(
                r#"
print("__test_fail_allocations_after_{budget}__")
value = read_text("sample.txt")
print("__test_restore_allocations__")
print(value)
print(read_text("sample.txt"))
"#
            ),
            Some(b"hello"),
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains(if budget < 3 {
                "Allocation(AllocError.OutOfMemory)"
            } else {
                "Result[str, IoError].Ok(\"hello\")"
            }),
            "{text}"
        );
        assert!(text.ends_with("Result[str, IoError].Ok(\"hello\")\n"));
    }
}

mod support;

#[test]
fn appending_creates_or_extends_and_preserves_exact_bytes() {
    for initial in [None, Some(&b"first\r\n"[..])] {
        let out = run_file(
            r#"
print(append_text("sample.txt", "é\n")).unwrap()
print(append_text("sample.txt", "")).unwrap()
print(append_text("sample.txt", "last")).unwrap()
print(read_text("sample.txt")).unwrap()
"#,
            initial,
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("Result[i64, IoError].Ok(2)\nResult[i64, IoError].Ok(0)\nResult[i64, IoError].Ok(4)\nResult[str, IoError].Ok(\"{}é\\nlast\")\n", if initial.is_some() { "first\\n" } else { "" }));
    }
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable(
        "append_text(\"sample.txt\", \"é\\0\\r\\n\")",
        &executable,
    )
    .unwrap();
    assert!(std::process::Command::new(executable)
        .current_dir(dir.path())
        .status()
        .unwrap()
        .success());
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        "é\0\r\n".as_bytes()
    );
    assert!(support::check_source("append_text(\"file\", 1)").is_err());
}

#[test]
fn file_arguments_run_once_in_source_order() {
    let out = run_file(
        r#"
def path() -> str:
    print("path").unwrap()
    "sample.txt"
def contents() -> str:
    print("contents").unwrap()
    "new"
print(append_text(path(), contents())).unwrap()
print(read_text("sample.txt")).unwrap()
"#,
        Some(b"old"),
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "path\ncontents\nResult[i64, IoError].Ok(3)\nResult[str, IoError].Ok(\"oldnew\")\n"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn append_path_failure_leaves_existing_contents_intact() {
    for budget in 0..=1 {
        let out = run_file(
            &format!(
                r#"
print("__test_fail_allocations_after_{budget}__").unwrap()
value = append_text("sample.txt", "new")
print("__test_restore_allocations__").unwrap()
print(value).unwrap()
print(read_text("sample.txt")).unwrap()
"#
            ),
            Some(b"old"),
        );
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.ends_with(if budget == 0 {
                "Result[str, IoError].Ok(\"old\")\n"
            } else {
                "Result[str, IoError].Ok(\"oldnew\")\n"
            }),
            "{text}"
        );
    }
}

#[test]
fn writing_replaces_and_closes_files_without_changing_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.txt");
    std::fs::write(&path, b"previous long contents").unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable(
        r#"
def save(path: &str, text: &str) -> Result[i64, IoError]:
    Ok(write_text(path, text)?)
path = "sample.txt"
text = "é🙂\0\r\n"
print(save(&path, &text)).unwrap()
print(len(text)).unwrap()
"#,
        &executable,
    )
    .unwrap();
    let output = std::process::Command::new(&executable)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Result[i64, IoError].Ok(5)\n5\n"
    );
    assert_eq!(std::fs::read(path).unwrap(), "é🙂\0\r\n".as_bytes());
    assert!(support::check_source("write_text(\"x\", 1)").is_err());
}

#[test]
fn writes_report_os_errors_and_empty_writes_truncate() {
    let output = run_file(
        "print(write_text(\"missing/child\", \"data\")).unwrap()",
        None,
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("IoError.System("));
    let output = run_file(
        "print(write_text(\"sample.txt\", \"\")).unwrap()\nprint(read_text(\"sample.txt\")).unwrap()",
        Some(b"old"),
    );
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Result[i64, IoError].Ok(0)\nResult[str, IoError].Ok(\"\")\n"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn write_failure_is_a_result() {
    let out = run_file("print(write_text(\"/dev/full\", \"data\")).unwrap()", None);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("IoError.System("));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn path_allocation_fails_before_truncation() {
    for budget in 0..=1 {
        let out = run_file(
            &format!(
                r#"
print("__test_fail_allocations_after_{budget}__").unwrap()
value = write_text("sample.txt", "new")
print("__test_restore_allocations__").unwrap()
print(value).unwrap()
print(read_text("sample.txt")).unwrap()
"#
            ),
            Some(b"old"),
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.ends_with(if budget == 0 {
                "Result[str, IoError].Ok(\"old\")\n"
            } else {
                "Result[str, IoError].Ok(\"new\")\n"
            }),
            "{text}"
        );
    }
}

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
        let out = run_file("print(read_text(\"sample.txt\")).unwrap()", Some(bytes));
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
        let out = run_file("print(read_text(\"sample.txt\")).unwrap()", bytes);
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains(expected));
    }
    let out = run_file(
        "print(read_text(\"sample.txt\\0suffix\")).unwrap()",
        Some(b"secret"),
    );
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
print("__test_fail_allocations_after_{budget}__").unwrap()
value = read_text("sample.txt")
print("__test_restore_allocations__").unwrap()
print(value).unwrap()
print(read_text("sample.txt")).unwrap()
"#
            ),
            Some(b"hello, world"),
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
                "Result[str, IoError].Ok(\"hello, world\")"
            }),
            "{text}"
        );
        assert!(text.ends_with("Result[str, IoError].Ok(\"hello, world\")\n"));
    }
}

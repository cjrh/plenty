mod support;
use std::process::Command;

fn run(source: &str, initial: Option<&[u8]>) -> (std::process::Output, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    if let Some(bytes) = initial {
        std::fs::write(dir.path().join("sample.txt"), bytes).unwrap();
    }
    let exe = dir.path().join("program");
    support::compile_source_to_executable(source, &exe).unwrap_or_else(|e| panic!("{source}\n{e}"));
    let out = Command::new(exe).current_dir(dir.path()).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (out, dir)
}
fn reject(source: &str, expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "expected {expected}, got {error}");
}

#[test]
fn files_are_owned_and_close_is_idempotent() {
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print(file.closed)
    print(file)
    file.close()?
    print(file.closed)
    file.close()?
    print(file)
    Ok(())
print(work())
"#,
        Some(b"text"),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "False\nFile(open)\nTrue\nFile(closed)\nResult[(), IoError].Ok(())\n"
    );
}

#[test]
fn scoped_reads_return_owned_text_and_advance_to_eof() {
    let (out, _) = run(
        r#"
def work() -> Result[str, IoError]:
    with open("sample.txt")? as file:
        text = file.read()?
        print(file.read()?)
        return Ok(text)
print(work())
"#,
        Some("é\r\nhello\r\0".as_bytes()),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "\nResult[str, IoError].Ok(\"é\\nhello\\n\\0\")\n"
    );
}

#[test]
fn borrowed_file_context_closes_on_return_and_read_errors() {
    for input in [&b"valid"[..], &b"\xff"[..]] {
        let (out, _) = run(
            r#"
def read(file: &mut File) -> Result[str, IoError]:
    with &mut file as stream:
        return stream.read()
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print(read(&mut file))
    print(file.closed)
    print(file.read())
    Ok(())
print(work())
"#,
            Some(input),
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("\nTrue\nResult[str, IoError].Err(IoError.System(0))"),
            "{text}"
        );
        if input == b"\xff" {
            assert!(text.contains("DataError.InvalidUtf8"), "{text}");
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn scoped_read_allocation_failures_release_file_without_allocating() {
    for budget in 0..=2 {
        let (out, _) = run(
            &format!(
                r#"
def read(file: File) -> Result[str, IoError]:
    with file as stream:
        return Ok(stream.read()?)
def work() -> Result[(), IoError]:
    file = open("sample.txt")?
    print("__test_fail_allocations_after_{budget}__")
    result = read(file)
    print("__test_restore_allocations__")
    print(result)
    Ok(())
print(work())
"#
            ),
            Some(b"text"),
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains(if budget < 2 {
                "OutOfMemory"
            } else {
                ".Ok(\"text\")"
            }),
            "{text}"
        );
    }
}

#[test]
fn file_reads_require_exclusive_access() {
    reject(
        "def read(file: &File) -> Result[str, IoError]:\n    file.read()\n",
        "shared reference as mutable",
    );
    reject(
        "def read(file: &mut File) -> Result[str, IoError]:\n    file.read(1)\n",
        "takes no arguments",
    );
}

#[test]
fn open_modes_create_truncate_or_preserve_and_errors_are_recoverable() {
    for (mode, expected) in [("r", &b"old"[..]), ("w", &b""[..]), ("a", &b"old"[..])] {
        let (_, dir) = run(
            &format!("value = open(\"sample.txt\", \"{mode}\")\n"),
            Some(b"old"),
        );
        assert_eq!(
            std::fs::read(dir.path().join("sample.txt")).unwrap(),
            expected
        );
    }
    for mode in ["w", "a"] {
        let (_, dir) = run(&format!("value = open(\"sample.txt\", \"{mode}\")\n"), None);
        assert_eq!(std::fs::read(dir.path().join("sample.txt")).unwrap(), b"");
    }
    let (out, _) = run(
        "print(open(\"missing.txt\"))\nprint(open(\"x\", \"bad\"))\nprint(open(\"x\\0y\"))\n",
        None,
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.matches(".Err(").count(), 3, "{text}");
}

#[test]
fn file_ownership_types_and_mutability_are_checked() {
    reject("def work() -> Result[(), IoError]:\n    file = open(\"x\")?\n    file.close()\n    Ok(())\n", "mut binding");
    reject(
        "def work() -> Result[(), IoError]:\n    file = open(\"x\")?\n    copy(file)\n    Ok(())\n",
        "cannot be copied",
    );
    reject("def work() -> Result[(), IoError]:\n    file = open(\"x\")?\n    other = file\n    print(file.closed)\n    Ok(())\n", "moved");
    reject("open(1)", "expected str");
    reject("open()", "takes a path");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn open_allocation_failures_precede_file_side_effects() {
    for budget in 0..=2 {
        let (out, dir) = run(
            &format!(
                r#"
print("__test_fail_allocations_after_{budget}__")
result = open("sample.txt", "w")
print("__test_restore_allocations__")
print(result)
"#
            ),
            Some(b"old"),
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains(if budget < 2 {
                "OutOfMemory"
            } else {
                ".Ok(File(open))"
            }),
            "{text}"
        );
        assert_eq!(
            std::fs::read(dir.path().join("sample.txt")).unwrap(),
            if budget < 2 { &b"old"[..] } else { &b""[..] }
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn close_and_drop_do_not_allocate() {
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print("__test_fail_allocations_after_0__")
    result = file.close()
    drop(file)
    print("__test_restore_allocations__")
    result
print(work())
"#,
        Some(b"old"),
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Result[(), IoError].Ok(())"));
}

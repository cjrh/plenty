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
fn sized_read_counts_unicode_and_translated_newlines() {
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt")? as file:
        print(file.read(0))
        print(file.read(2))
        print(file.read(1))
        print(file.read(-1))
    Ok(())
print(work())
"#,
        Some("é🦀\r\nlast".as_bytes()),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Result[str, IoError].Ok(\"\")\nResult[str, IoError].Ok(\"é🦀\")\nResult[str, IoError].Ok(\"\\n\")\nResult[str, IoError].Ok(\"last\")\nResult[(), IoError].Ok(())\n");
}

#[test]
fn sized_readline_stops_at_limit_or_newline() {
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt")? as file:
        print(file.readline(1))
        print(file.readline(0))
        print(file.readline(20))
        print(file.readline(-1))
        print(file.read(1))
    Ok(())
print(work())
"#,
        Some("é🦀\r\nnext\nz".as_bytes()),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Result[str, IoError].Ok(\"é\")\nResult[str, IoError].Ok(\"\")\nResult[str, IoError].Ok(\"🦀\\n\")\nResult[str, IoError].Ok(\"next\\n\")\nResult[str, IoError].Ok(\"z\")\nResult[(), IoError].Ok(())\n");
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
fn file_capabilities_use_shared_references_and_validate_closed_state() {
    let (out, _) = run(
        r#"
def inspect(file: &File) -> Result[(), IoError]:
    print(file.readable()?)
    print(file.writable()?)
    Ok(())
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    inspect(&file)?
    file.close()?
    print(file.readable())
    with open("sample.txt", "w")? as stream:
        inspect(&stream)?
        print(stream.read(0))
    Ok(())
print(work())
"#,
        Some(b"hello"),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "True\nFalse\nResult[bool, IoError].Err(IoError.System(0))\nFalse\nTrue\nResult[str, IoError].Err(IoError.System(0))\nResult[(), IoError].Ok(())\n");
}

#[test]
fn readline_preserves_line_endings_and_distinguishes_empty_lines_from_eof() {
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt")? as file:
        for n in range(6):
            print(file.readline())
    Ok(())
print(work())
"#,
        Some("é\r\n\r\n\n\0last".as_bytes()),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Result[str, IoError].Ok(\"é\\n\")\nResult[str, IoError].Ok(\"\\n\")\nResult[str, IoError].Ok(\"\\n\")\nResult[str, IoError].Ok(\"\\0last\")\nResult[str, IoError].Ok(\"\")\nResult[str, IoError].Ok(\"\")\nResult[(), IoError].Ok(())\n");
}

#[test]
fn readline_and_read_share_newline_state_and_recover_after_invalid_utf8() {
    for input in [&b"one\r\ntwo\rthree"[..], &b"one\rtwo\rthree"[..]] {
        let (out, _) = run(
            r#"
def work() -> Result[str, IoError]:
    with open("sample.txt")? as file:
        first = file.readline()?
        return file.read()
print(work())
"#,
            Some(input),
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "Result[str, IoError].Ok(\"two\\nthree\")\n"
        );
    }
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt")? as file:
        print(file.readline())
        print(file.readline())
        file.close()?
        print(file.readline())
    Ok(())
print(work())
"#,
        Some(b"\xff\r\nok\n"),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("DataError.InvalidUtf8"), "{text}");
    assert!(text.contains(".Ok(\"ok\\n\")"), "{text}");
    assert!(text.contains("IoError.System(0)"), "{text}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn bounded_reads_report_allocation_failure_and_remain_closable() {
    for method in ["read", "readline"] {
        for budget in 0..=2 {
            let (out, _) = run(
                &format!(
                    r#"
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print("__test_fail_allocations_after_{budget}__")
    result = file.{method}(2)
    file.close()?
    print("__test_restore_allocations__")
    print(result)
    print(file.closed)
    Ok(())
print(work())
"#
                ),
                Some("é🦀".as_bytes()),
            );
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                text.contains(if budget < 2 {
                    "OutOfMemory"
                } else {
                    ".Ok(\"é🦀\")"
                }),
                "{text}"
            );
            assert!(text.contains("\nTrue\n"), "{text}");
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn readline_allocation_failure_preserves_a_valid_closable_owner() {
    for budget in 0..=2 {
        let (out, _) = run(
            &format!(
                r#"
def read(file: &mut File) -> Result[str, IoError]:
    with &mut file as stream:
        return Ok(stream.readline()?)
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print("__test_fail_allocations_after_{budget}__")
    result = read(&mut file)
    print("__test_restore_allocations__")
    print(result)
    print(file.closed)
    Ok(())
print(work())
"#
            ),
            Some(b"a\r\nb\n"),
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains(if budget < 2 {
                "OutOfMemory"
            } else {
                ".Ok(\"a\\n\")"
            }),
            "{text}"
        );
        assert!(text.contains("\nTrue\n"), "{text}");
    }
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
        "def read(file: &mut File) -> Result[str, IoError]:\n    file.read(1, 2)\n",
        "takes one argument",
    );
}

#[test]
fn writes_preserve_exact_bytes_and_append_mode() {
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt", "w")? as file:
        print(file.write("é\0\r\n")?)
        file.flush()?
        file.sync()?
        file.close()?
    with open("sample.txt", "a")? as file:
        print(file.write("🙂")?)
    Ok(())
print(work())
"#,
        Some(b"old contents"),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "4\n1\nResult[(), IoError].Ok(())\n"
    );
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        "é\0\r\n🙂".as_bytes()
    );
}

#[test]
fn writes_evaluate_text_once_before_exclusive_access() {
    let (out, dir) = run(
        r#"
def text(file: &File) -> str:
    print(file.closed)
    "hello"
def work() -> Result[(), IoError]:
    with open("sample.txt", "w")? as file:
        print(file.write(text(file))?)
    Ok(())
print(work())
"#,
        None,
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "False\n5\nResult[(), IoError].Ok(())\n"
    );
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"hello"
    );
}

#[test]
fn wrong_mode_closed_and_os_write_failures_are_results() {
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print(file.write(""))
    file.close()?
    print(file.write("x"))
    print(file.flush())
    print(file.sync())
    with open("/dev/full", "w")? as full:
        print(full.write("x"))
    Ok(())
print(work())
"#,
        Some(b"old"),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.matches(".Err(").count(), 5, "{text}");
    reject(
        "def write(file: &mut File) -> Result[i64, IoError]:\n    file.write(1)\n",
        "expected str",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn writes_flushes_and_sync_need_no_runtime_allocation() {
    let (out, dir) = run(
        r#"
def write(file: File) -> Result[(), IoError]:
    with file as stream:
        stream.write("hello")?
        stream.flush()?
        stream.sync()?
    Ok(())
def work() -> Result[(), IoError]:
    file = open("sample.txt", "w")?
    print("__test_fail_allocations_after_0__")
    result = write(file)
    print("__test_restore_allocations__")
    result
print(work())
"#,
        None,
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Result[(), IoError].Ok(())"));
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"hello"
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

#[test]
fn text_positions_restore_unicode_and_pending_crlf_state() {
    for input in ["é\r\n🦀end", "é\r🦀end"] {
        let (out, _) = run(
            r#"
def work() -> Result[(), IoError]:
    with open("sample.txt")? as file:
        file.readline()?
        position = file.tell()?
        print(file.read(1)?)
        file.seek(position)?
        print(file.read()?)
        file.seek(0u64)?
        print(file.read(1)?)
        file.close()?
        print(file.tell())
        print(file.seek(position))
    Ok(())
print(work())
"#,
            Some(input.as_bytes()),
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "🦀\n🦀end\né\nResult[u64, IoError].Err(IoError.System(0))\nResult[(), IoError].Err(IoError.System(0))\nResult[(), IoError].Ok(())\n");
    }
}

#[test]
fn seeking_an_append_stream_does_not_reposition_writes() {
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt", "a+")? as file:
        file.seek(0u64)?
        print(file.read(1)?)
        file.seek(0u64)?
        file.write("!")?
    Ok(())
print(work())
"#,
        Some(b"abc"),
    );
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("a\n"));
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"abc!"
    );
    reject(
        "def bad(file: &File) -> Result[u64, IoError]:\n    file.tell()",
        "shared reference as mutable",
    );
    reject(
        "def bad(file: &mut File) -> Result[(), IoError]:\n    file.seek(1)",
        "expected u64",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn saved_position_operations_do_not_allocate() {
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt")? as file:
        print("__test_fail_allocations_after_0__")
        position = file.tell()?
        result = file.seek(position)
        print("__test_restore_allocations__")
        print(result)
    Ok(())
print(work())
"#,
        Some(b"ok"),
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains(".Err("));
}

#[test]
fn exclusive_creation_never_replaces_an_existing_file() {
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt", "x")? as file:
        print(file.write("created")?)
    print(open("sample.txt", "x"))
    Ok(())
print(work())
"#,
        None,
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains(".Err(IoError.System(17))"));
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"created"
    );
    let (out, dir) = run("print(open(\"sample.txt\", \"x\"))", Some(b"keep"));
    assert!(String::from_utf8_lossy(&out.stdout).contains(".Err("));
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"keep"
    );
}

#[test]
fn update_modes_read_write_and_preserve_their_creation_contracts() {
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt", "r+")? as file:
        print(file.readable()?)
        print(file.writable()?)
        print(file.read(2)?)
        file.write("X")?
        print(file.read()?)
    with open("sample.txt", "a+")? as file:
        print(file.read())
        file.write("!")?
    Ok(())
print(work())
"#,
        Some(b"abcde"),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "True\nTrue\nab\nde\nResult[str, IoError].Ok(\"\")\nResult[(), IoError].Ok(())\n"
    );
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"abXde!"
    );
    for mode in ["w+", "x+"] {
        let (_, dir) = run(&format!("def work() -> Result[(), IoError]:\n    with open(\"sample.txt\", \"{mode}\")? as file:\n        file.write(\"ok\")?\n    Ok(())\nprint(work())"), None);
        assert_eq!(std::fs::read(dir.path().join("sample.txt")).unwrap(), b"ok");
    }
    let (out, dir) = run("print(open(\"sample.txt\", \"r+\"))", None);
    assert!(String::from_utf8_lossy(&out.stdout).contains(".Err("));
    assert!(!dir.path().join("sample.txt").exists());
    let (_, dir) = run("file = open(\"sample.txt\", \"w+\")", Some(b"old"));
    assert_eq!(std::fs::read(dir.path().join("sample.txt")).unwrap(), b"");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn exclusive_creation_allocation_failures_leave_no_file() {
    for budget in 0..=2 {
        let (out, dir) = run(
            &format!(
                r#"
print("__test_fail_allocations_after_{budget}__")
result = open("sample.txt", "x")
print("__test_restore_allocations__")
print(result)
"#
            ),
            None,
        );
        assert_eq!(dir.path().join("sample.txt").exists(), budget == 2);
        assert!(
            String::from_utf8_lossy(&out.stdout).contains(if budget < 2 {
                "OutOfMemory"
            } else {
                ".Ok(File(open))"
            })
        );
    }
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

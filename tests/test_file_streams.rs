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
        print(str.repr(file.read(0)).unwrap()).unwrap()
        print(str.repr(file.read(2)).unwrap()).unwrap()
        print(str.repr(file.read(1)).unwrap()).unwrap()
        print(str.repr(file.read(-1)).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
        print(str.repr(file.readline(1)).unwrap()).unwrap()
        print(str.repr(file.readline(0)).unwrap()).unwrap()
        print(str.repr(file.readline(20)).unwrap()).unwrap()
        print(str.repr(file.readline(-1)).unwrap()).unwrap()
        print(str.repr(file.read(1)).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
    print(file.closed).unwrap()
    print(file).unwrap()
    file.close()?
    print(file.closed).unwrap()
    file.close()?
    print(file).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
    print(file.readable()?).unwrap()
    print(file.writable()?).unwrap()
    Ok(())
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    inspect(&file)?
    file.close()?
    print(str.repr(file.readable()).unwrap()).unwrap()
    with open("sample.txt", "w")? as stream:
        inspect(&stream)?
        print(str.repr(stream.read(0)).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
            print(str.repr(file.readline()).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
print(str.repr(work()).unwrap()).unwrap()
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
        print(str.repr(file.readline()).unwrap()).unwrap()
        print(str.repr(file.readline()).unwrap()).unwrap()
        file.close()?
        print(str.repr(file.readline()).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = file.{method}(3)
    file.close()?
    print("__test_restore_allocations__").unwrap()
    print(str.repr(result).unwrap()).unwrap()
    print(file.closed).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#
                ),
                Some("é🦀é".as_bytes()),
            );
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                text.contains(if budget < 2 {
                    "OutOfMemory"
                } else {
                    ".Ok(\"é🦀é\")"
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
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = read(&mut file)
    print("__test_restore_allocations__").unwrap()
    print(str.repr(result).unwrap()).unwrap()
    print(file.closed).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#
            ),
            Some(b"abcdefg\nb\n"),
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains(if budget < 2 {
                "OutOfMemory"
            } else {
                ".Ok(\"abcdefg\\n\")"
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
        print(file.read()?).unwrap()
        return Ok(text)
print(str.repr(work()).unwrap()).unwrap()
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
    print(str.repr(read(&mut file)).unwrap()).unwrap()
    print(file.closed).unwrap()
    print(str.repr(file.read()).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = read(file)
    print("__test_restore_allocations__").unwrap()
    print(str.repr(result).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#
            ),
            Some(b"text file"),
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains(if budget < 2 {
                "OutOfMemory"
            } else {
                ".Ok(\"text file\")"
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
        print(file.write("é\0\r\n")?).unwrap()
        file.flush()?
        file.sync()?
        file.close()?
    with open("sample.txt", "a")? as file:
        print(file.write("🙂")?).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
    print(file.closed).unwrap()
    "hello"
def work() -> Result[(), IoError]:
    with open("sample.txt", "w")? as file:
        print(file.write(text(file))?).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
    print(str.repr(file.write("")).unwrap()).unwrap()
    file.close()?
    print(str.repr(file.write("x")).unwrap()).unwrap()
    print(str.repr(file.flush()).unwrap()).unwrap()
    print(str.repr(file.sync()).unwrap()).unwrap()
    with open("/dev/full", "w")? as full:
        print(str.repr(full.write("x")).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
    print("__test_fail_allocations_after_0__").unwrap()
    result = write(file)
    print("__test_restore_allocations__").unwrap()
    result
print(str.repr(work()).unwrap()).unwrap()
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
        "print(str.repr(open(\"missing.txt\")).unwrap()).unwrap()\nprint(str.repr(open(\"x\", \"bad\")).unwrap()).unwrap()\nprint(str.repr(open(\"x\\0y\")).unwrap()).unwrap()\n",
        None,
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.matches(".Err(").count(), 3, "{text}");
}

#[test]
fn file_ownership_types_and_mutability_are_checked() {
    reject("def work() -> Result[(), IoError]:\n    file = open(\"x\")?\n    file.close()\n    Ok(())\n", "mut binding");
    reject(
        "def work() -> Result[(), IoError]:\n    file = open(\"x\")?\n    copy(file).unwrap()\n    Ok(())\n",
        "cannot be copied",
    );
    reject("def work() -> Result[(), IoError]:\n    file = open(\"x\")?\n    other = file\n    print(file.closed).unwrap()\n    Ok(())\n", "moved");
    reject("open(1)", "expected str");
    reject("open()", "takes a path");
}

#[test]
fn writelines_borrows_strings_and_preserves_exact_contents() {
    let (out, dir) = run(
        r#"
def save(file: &mut File, lines: &list[str]) -> Result[(), IoError]:
    file.writelines(lines)
def work() -> Result[(), IoError]:
    lines = ["é\r\n", "\0", "🦀"].unwrap()
    with open("sample.txt", "w")? as file:
        save(&mut file, &lines)?
        file.writelines([].unwrap())?
        file.writelines(["!"].unwrap())?
    print(lines).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        None,
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("[\"é\\r\\n\", \"\\0\", \"🦀\"]"));
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        "é\r\n\0🦀!".as_bytes()
    );
    reject(
        "def bad(file: &mut File) -> Result[(), IoError]:\n    file.writelines([1].unwrap())",
        "expected list[str]",
    );
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print(str.repr(file.writelines([].unwrap())).unwrap()).unwrap()
    file.close()?
    print(str.repr(file.writelines([].unwrap())).unwrap()).unwrap()
    with open("/dev/full", "w")? as full:
        print(str.repr(full.writelines(["x"].unwrap())).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        Some(b"existing"),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout)
            .matches(".Err(")
            .count(),
        3
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn writelines_needs_no_allocation_after_argument_construction() {
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    lines = ["one\n", "two"].unwrap()
    with open("sample.txt", "w")? as file:
        print("__test_fail_allocations_after_0__").unwrap()
        result = file.writelines(lines)
        print("__test_restore_allocations__").unwrap()
        print(str.repr(result).unwrap()).unwrap()
    print(lines).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        None,
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains(".Err("));
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"one\ntwo"
    );
}

#[test]
fn readlines_owns_normalized_lines_and_distinguishes_eof() {
    let (out, _) = run(
        r#"
def work() -> Result[list[str], IoError]:
    with open("sample.txt")? as file:
        file.read(1)?
        lines = file.readlines()?
        print(file.readlines()?).unwrap()
        return Ok(lines)
print(str.repr(work()).unwrap()).unwrap()
"#,
        Some("xé\r\n\r🦀\nlast".as_bytes()),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "[]\nResult[list[str], IoError].Ok([\"é\\n\", \"\\n\", \"🦀\\n\", \"last\"])\n"
    );
    let (out, _) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt")? as file:
        print(str.repr(file.readlines()).unwrap()).unwrap()
        print(str.repr(file.readline()).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        Some(b"ok\n\xff\nnext\n"),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("DataError.InvalidUtf8"), "{text}");
    assert!(text.contains(".Ok(\"next\\n\")"), "{text}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn readlines_cleans_partial_lists_under_allocation_failures() {
    let mut succeeded = false;
    for budget in 0..=12 {
        let (out, _) = run(
            &format!(
                r#"
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = file.readlines()
    file.close()?
    print("__test_restore_allocations__").unwrap()
    print(str.repr(result).unwrap()).unwrap()
    print(file.closed).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#
            ),
            Some(b"one\ntwo\nthree"),
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("\nTrue\n"), "{text}");
        assert!(
            text.contains("OutOfMemory")
                || text.contains(".Ok([\"one\\n\", \"two\\n\", \"three\"])"),
            "{text}"
        );
        succeeded |= text.contains(".Ok([");
    }
    assert!(succeeded);
}

#[test]
fn truncate_uses_byte_lengths_without_moving_the_cursor() {
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt", "r+")? as file:
        file.read(1)?
        before = file.tell()?
        print(file.truncate()?).unwrap()
        print(file.tell()? == before).unwrap()
        print(file.truncate(4)?).unwrap()
        print(str.repr(file.read()).unwrap()).unwrap()
        print(str.repr(file.truncate(-1)).unwrap()).unwrap()
        print(file.truncate(1)?).unwrap()
        file.seek(0u64)?
        print(str.repr(file.read()).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        Some("éhello".as_bytes()),
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("2\nTrue\n4\nResult[str, IoError].Ok(\"\\0\\0\")\n"),
        "{text}"
    );
    assert!(text.contains("IoError.System(0)"), "{text}");
    assert!(text.contains("DataError.InvalidUtf8"), "{text}");
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"\xc3"
    );
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    mut file = open("sample.txt")?
    print(str.repr(file.truncate(0)).unwrap()).unwrap()
    file.close()?
    print(str.repr(file.truncate()).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        Some(b"keep"),
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout)
            .matches(".Err(")
            .count(),
        2
    );
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"keep"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn truncate_does_not_allocate() {
    let (out, dir) = run(
        r#"
def work() -> Result[(), IoError]:
    with open("sample.txt", "r+")? as file:
        print("__test_fail_allocations_after_0__").unwrap()
        result = file.truncate(2)
        print("__test_restore_allocations__").unwrap()
        print(str.repr(result).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        Some(b"abcdef"),
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Result[i64, IoError].Ok(2)"));
    assert_eq!(std::fs::read(dir.path().join("sample.txt")).unwrap(), b"ab");
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
        print(file.read(1)?).unwrap()
        file.seek(position)?
        print(file.read()?).unwrap()
        file.seek(0u64)?
        print(file.read(1)?).unwrap()
        file.close()?
        print(str.repr(file.tell()).unwrap()).unwrap()
        print(str.repr(file.seek(position)).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
        print(file.read(1)?).unwrap()
        file.seek(0u64)?
        file.write("!")?
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
        print("__test_fail_allocations_after_0__").unwrap()
        position = file.tell()?
        result = file.seek(position)
        print("__test_restore_allocations__").unwrap()
        print(str.repr(result).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
        print(file.write("created")?).unwrap()
    print(str.repr(open("sample.txt", "x")).unwrap()).unwrap()
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
"#,
        None,
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains(".Err(IoError.System(17))"));
    assert_eq!(
        std::fs::read(dir.path().join("sample.txt")).unwrap(),
        b"created"
    );
    let (out, dir) = run(
        "print(str.repr(open(\"sample.txt\", \"x\")).unwrap()).unwrap()",
        Some(b"keep"),
    );
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
        print(file.readable()?).unwrap()
        print(file.writable()?).unwrap()
        print(file.read(2)?).unwrap()
        file.write("X")?
        print(file.read()?).unwrap()
    with open("sample.txt", "a+")? as file:
        print(str.repr(file.read()).unwrap()).unwrap()
        file.write("!")?
    Ok(())
print(str.repr(work()).unwrap()).unwrap()
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
        let (_, dir) = run(&format!("def work() -> Result[(), IoError]:\n    with open(\"sample.txt\", \"{mode}\")? as file:\n        file.write(\"ok\")?\n    Ok(())\nprint(str.repr(work()).unwrap()).unwrap()"), None);
        assert_eq!(std::fs::read(dir.path().join("sample.txt")).unwrap(), b"ok");
    }
    let (out, dir) = run(
        "print(str.repr(open(\"sample.txt\", \"r+\")).unwrap()).unwrap()",
        None,
    );
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
print("__test_fail_allocations_after_{budget}__").unwrap()
result = open("sample.txt", "x")
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
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
print("__test_fail_allocations_after_{budget}__").unwrap()
result = open("sample.txt", "w")
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
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
    print("__test_fail_allocations_after_0__").unwrap()
    result = file.close()
    drop(file)
    print("__test_restore_allocations__").unwrap()
    result
print(str.repr(work()).unwrap()).unwrap()
"#,
        Some(b"old"),
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("Result[(), IoError].Ok(())"));
}

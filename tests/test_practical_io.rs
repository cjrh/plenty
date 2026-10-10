mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn with_input(source: &str, input: &[u8]) -> std::process::Output {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable(source, &executable).unwrap();
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn input_distinguishes_empty_lines_eof_and_invalid_utf8() {
    let output = with_input(
        "print(str.repr(input()).unwrap()).unwrap()\nprint(str.repr(input()).unwrap()).unwrap()\nprint(str.repr(input()).unwrap()).unwrap()\nprint(str.repr(input()).unwrap()).unwrap()",
        "é\0\r\n\nlast".as_bytes(),
    );
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Result[Option[str], IoError].Ok(Option[str].Some(\"é\\0\"))\nResult[Option[str], IoError].Ok(Option[str].Some(\"\"))\nResult[Option[str], IoError].Ok(Option[str].Some(\"last\"))\nResult[Option[str], IoError].Ok(Option[str].Nothing)\n");
    let output = with_input("print(str.repr(input()).unwrap()).unwrap()", b"\xff\n");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Result[Option[str], IoError].Err(IoError.Data(DataError.InvalidUtf8))\n"
    );
    assert!(support::check_source("input(1)").is_err());
}

#[cfg(feature = "runtime-checks")]
#[test]
fn input_allocation_failure_is_recoverable() {
    for budget in 0..=2 {
        let output = with_input(
            &format!(
                r#"
print("__test_fail_allocations_after_{budget}__").unwrap()
a = input()
print("__test_restore_allocations__").unwrap()
print(str.repr(a).unwrap()).unwrap()
"#
            ),
            b"abcdefgh\n",
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains(if budget < 2 {
                "Allocation(AllocError.OutOfMemory)"
            } else {
                "Some(\"abcdefgh\")"
            }),
            "{text}"
        );
    }
}

#[test]
fn writes_exact_text_and_returns_character_count() {
    native(
        r#"
def send(text: &str) -> Result[i64, IoError]:
    Ok(write_stdout(text)?)
text = "hé🙂\0"
result = send(&text)
print(str.repr(result).unwrap()).unwrap()
print(text).unwrap()
print(str.repr(write_stdout("")).unwrap()).unwrap()
"#,
        "hé🙂\0Result[i64, IoError].Ok(4)\nhé🙂\0\nResult[i64, IoError].Ok(0)\n",
    );
}

#[test]
fn io_errors_are_constructible_and_matchable() {
    native(r#"
def error() -> Result[i64, IoError]:
    Err(IoError.Data(DataError.Allocation(AllocError.OutOfMemory)))
print(str.repr(error()).unwrap()).unwrap()
print(IoError.System(5i32)).unwrap()
print(IoError.Data(DataError.InvalidUtf8)).unwrap()
"#, "Result[i64, IoError].Err(IoError.Data(DataError.Allocation(AllocError.OutOfMemory)))\nIoError.System(5)\nIoError.Data(DataError.InvalidUtf8)\n");
}

#[test]
fn write_checks_arguments() {
    for source in [
        "write_stdout()",
        "write_stdout(1)",
        "write_stdout(\"a\", \"b\")",
    ] {
        assert!(support::check_source(source).is_err());
    }
}

#[test]
fn arguments_preserve_order_unicode_and_independent_owners() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable(
        r#"
def inspect() -> Result[(), IoError]:
    mut first = args()?
    second = args()?
    first.clear()
    print(len(first)).unwrap()
    print(len(second)).unwrap()
    print(second[1]).unwrap()
    print(second[2]).unwrap()
    print(second[3]).unwrap()
    Ok(())
def main() -> ():
    print(str.repr(inspect()).unwrap()).unwrap()
"#,
        &executable,
    )
    .unwrap();
    let output = std::process::Command::new(executable)
        .args(["hello world", "é🙂", ""])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0\n4\nhello world\né🙂\n\nResult[(), IoError].Ok(())\n"
    );
}

#[cfg(unix)]
#[test]
fn arguments_reject_invalid_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable("print(str.repr(args()).unwrap()).unwrap()", &executable)
        .unwrap();
    let output = std::process::Command::new(executable)
        .arg(std::ffi::OsStr::from_bytes(b"\xff"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("IoError.Data(DataError.InvalidUtf8)"));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn argument_snapshot_cleans_up_every_failed_allocation() {
    for budget in 0..=3 {
        let output = support::run(&format!(
            r#"
print("__test_fail_allocations_after_{budget}__").unwrap()
a = args()
print("__test_restore_allocations__").unwrap()
match a:
    case Ok(items):
        print(len(items)).unwrap()
    case Err(error):
        print(error).unwrap()
"#
        ));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains(if budget < 3 {
                "Allocation(AllocError.OutOfMemory)"
            } else {
                "\n1\n"
            }),
            "{text}"
        );
    }
}

#[test]
fn stderr_is_separate_and_flush_returns_unit_result() {
    let output = support::run(
        r#"
print(str.repr(write_stderr("diagnostic é\n")).unwrap()).unwrap()
print(str.repr(flush_stderr()).unwrap()).unwrap()
print(str.repr(flush_stdout()).unwrap()).unwrap()
"#,
    );
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "diagnostic é\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Result[i64, IoError].Ok(13)\nResult[(), IoError].Ok(())\nResult[(), IoError].Ok(())\n"
    );
    assert!(support::check_source("flush_stdout(1)").is_err());
    assert!(support::check_source("write_stderr(1)").is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn buffered_failure_is_reported_by_flush() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable(
        r#"
def main() -> i32:
    drop(write_stdout("buffered"))
    match flush_stdout():
        case Ok(unit):
            1i32
        case Err(error):
            0i32
"#,
        &executable,
    )
    .unwrap();
    assert!(std::process::Command::new(executable)
        .stdout(
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap()
        )
        .status()
        .unwrap()
        .success());
}

#[cfg(target_os = "linux")]
#[test]
fn stdout_failure_is_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    support::compile_source_to_executable(
        r#"
def main() -> i32:
    match write_stdout("line\n"):
        case Ok(count):
            1i32
        case Err(error):
            0i32
"#,
        &executable,
    )
    .unwrap();
    let output = std::process::Command::new(executable)
        .stdout(
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        )
        .status()
        .unwrap();
    assert!(output.success());
}

#[cfg(feature = "runtime-checks")]
#[test]
fn output_and_io_error_construction_do_not_allocate() {
    let output = support::run(
        r#"
print("__test_fail_allocations_after_0__").unwrap()
a = write_stdout("hello\n")
b = IoError.Data(DataError.Allocation(AllocError.OutOfMemory))
print("__test_restore_allocations__").unwrap()
print(str.repr(a).unwrap()).unwrap()
print(b).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Result[i64, IoError].Ok(6)"));
}

mod support;

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn input(source: &str, data: &[u8]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("program");
    support::compile_source_to_executable(source, &binary).unwrap();
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Allocation failure can return before stdin is read. Preserve the child's
    // status and diagnostics instead of failing first on a closed pipe.
    if let Err(error) = child.stdin.take().unwrap().write_all(data) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

#[test]
fn records_preserve_bytes_empty_records_and_unterminated_eof() {
    let data = b"\0space tab\tCR\rLF\n\xff\0last";
    let output = input(
        r#"
def main() -> Result[(), IoError]:
    terminator = encode_utf8("\0")?
    while True:
        match input_bytes(0u8)?:
            case Some(record):
                write_stdout_bytes(record)?
                write_stdout_bytes(&terminator)?
            case Nothing:
                break
    flush_stdout()?
    Ok(())
"#,
        data,
    );
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(output.stdout, [data.as_slice(), b"\0"].concat());
    assert!(output.stderr.is_empty());
    let output = input(
        "print(len(input_bytes(255u8).unwrap().unwrap())).unwrap()",
        &[0, 1, 254, 255],
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"3\n");
}

#[test]
fn explicit_utf8_conversion_is_strict_and_observes_owners() {
    let output = support::run(
        r#"
text = "é🙂\r\n\0"
mut bytes = encode_utf8(text).unwrap()
print(len(bytes)).unwrap()
print(decode_utf8(&bytes).unwrap() == text).unwrap()
print(len(bytes)).unwrap()
bytes.clear()
print(len(encode_utf8(text).unwrap())).unwrap()
print(str.repr(decode_utf8([255u8].unwrap())).unwrap()).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{:?}: {:?}",
        output.status,
        output.stderr
    );
    assert_eq!(
        output.stdout,
        b"9\nTrue\n9\n9\nResult[str, IoError].Err(IoError.Data(DataError.InvalidUtf8))\n"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn raw_arguments_and_file_paths_roundtrip_every_byte() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("program");
    support::compile_source_to_executable(
        r#"
def main() -> Result[(), IoError]:
    mut first = args_bytes()?
    second = args_bytes()?
    first.clear()
    data = read_bytes(&second[1])?
    write_bytes(&second[2], &data)?
    write_stdout_bytes(&second[3])?
    write_stdout_bytes(&data)?
    write_stderr_bytes(&data)?
    flush_stdout()?
    flush_stderr()?
    Ok(())
"#,
        &binary,
    )
    .unwrap();
    let original = dir
        .path()
        .join(std::ffi::OsStr::from_bytes(b"space \t\r\n\xff"));
    let destination = dir.path().join(std::ffi::OsStr::from_bytes(b"copy\xfe"));
    let data: Vec<u8> = (0..=255).cycle().take(20000).collect();
    std::fs::write(&original, &data).unwrap();
    let output = Command::new(binary)
        .arg(original)
        .arg(&destination)
        .arg(std::ffi::OsStr::from_bytes(b"\xff\r\n\t"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{:?}: {:?}",
        output.status,
        output.stderr
    );
    assert_eq!(output.stdout, [b"\xff\r\n\t".as_slice(), &data].concat());
    assert_eq!(output.stderr, data);
    assert_eq!(std::fs::read(destination).unwrap(), data);
}

#[test]
fn nul_paths_and_wrong_types_are_rejected() {
    let output = support::run(
        r#"
print(str.repr(read_bytes([0u8].unwrap())).unwrap()).unwrap()
print(str.repr(write_bytes([0u8].unwrap(), [255u8].unwrap())).unwrap()).unwrap()
"#,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"Result[list[u8], IoError].Err(IoError.InvalidInput)\nResult[i64, IoError].Err(IoError.InvalidInput)\n");
    for source in [
        "args_bytes(1)",
        "input_bytes()",
        "input_bytes(-1)",
        "encode_utf8(1)",
        "decode_utf8(\"text\")",
        "write_stdout_bytes(\"text\")",
        "read_bytes(\"path\")",
        "write_bytes([0u8].unwrap())",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_byte_allocations_clean_up_nested_owners() {
    for operation in [
        "args_bytes()",
        "encode_utf8(\"payload\")",
        "input_bytes(0u8)",
        "decode_utf8([255u8].unwrap())",
    ] {
        for budget in 0..=6 {
            let source = format!(
                "print(\"__test_fail_allocations_after_{budget}__\").unwrap()\na = {operation}\nprint(\"__test_restore_allocations__\").unwrap()\nprint(str.repr(a).unwrap()).unwrap()"
            );
            // Construct the decode input before injecting allocation failures.
            let source = source.replace(
                "a = decode_utf8([255u8].unwrap())",
                "a = decode_utf8(bytes)",
            );
            let source = format!("bytes = [255u8].unwrap()\n{source}");
            let output = input(&source, b"payload\0");
            assert!(
                output.status.success(),
                "{operation}, {budget}: {:?}",
                output.stderr
            );
            assert!(
                output.stderr.is_empty(),
                "{operation}, {budget}: {:?}",
                output.stderr
            );
            if budget == 0 {
                assert!(String::from_utf8_lossy(&output.stdout).contains("OutOfMemory"));
            }
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn binary_writes_do_not_allocate_and_return_byte_counts() {
    let output = support::run(
        r#"
bytes = encode_utf8("é\n").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = write_stdout_bytes(&bytes)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(len(bytes)).unwrap()
"#,
    );
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(String::from_utf8_lossy(&output.stdout).ends_with("Result[i64, IoError].Ok(3)\n3\n"));
}

#[cfg(target_os = "linux")]
#[test]
fn binary_write_errors_are_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("program");
    support::compile_source_to_executable(
        r#"
def main() -> i32:
    data = encode_utf8("raw\n").unwrap()
    match write_stdout_bytes(&data):
        case Ok(_):
            1i32
        case Err(_):
            0i32
"#,
        &binary,
    )
    .unwrap();
    assert!(Command::new(binary)
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

#[cfg(all(target_os = "linux", feature = "runtime-checks"))]
#[test]
fn file_allocation_failures_preserve_destination_and_release_partial_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data");
    let binary = dir.path().join("program");
    for budget in 0..=6 {
        std::fs::write(&path, b"original\xff\r\n").unwrap();
        let source = format!(
            r#"
def main() -> ():
    argv = args_bytes().unwrap()
    data = encode_utf8("replacement").unwrap()
    print("__test_fail_allocations_after_{budget}__").unwrap()
    written = write_bytes(&argv[1], &data)
    print("__test_restore_allocations__").unwrap()
    print(str.repr(written).unwrap()).unwrap()
    print("__test_fail_allocations_after_{budget}__").unwrap()
    read = read_bytes(&argv[1])
    print("__test_restore_allocations__").unwrap()
    print(str.repr(read).unwrap()).unwrap()
"#
        );
        support::compile_source_to_executable(&source, &binary).unwrap();
        let output = Command::new(&binary).arg(&path).output().unwrap();
        assert!(output.status.success(), "{budget}: {:?}", output.stderr);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            if budget < 2 {
                b"original\xff\r\n".as_slice()
            } else {
                b"replacement".as_slice()
            }
        );
        if budget < 2 {
            assert!(String::from_utf8_lossy(&output.stdout).contains("OutOfMemory"));
        }
    }
}

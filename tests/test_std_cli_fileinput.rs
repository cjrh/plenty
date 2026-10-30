use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod support;

fn compile(directory: &Path, body: &str) -> PathBuf {
    let source = directory.join("fileinput.plenty");
    let executable = directory.join("fileinput");
    std::fs::write(
        &source,
        format!(
            r#"
import std.cli.fileinput as fileinput
import std.io as io
import std.path as paths
import std.bytes as buffers
import std.fs as fs
from std.cli.fileinput import FileInputError
from std.io import StreamError
from std.bytes import BytesError

def descriptors() -> Result[i64, IoError]:
    mut count = 0
    for entry in fs.entries(fs.read_dir(&paths.from_text("/proc/self/fd")?)?):
        drop(entry?)
        count += 1
    Ok(count)

def propagate_file_error() -> Result[(), IoError | AllocError | FileInputError]:
    for line in fileinput.files([paths.from_text("invalid")?]?):
        drop(line?)
    Ok(())

def return_early() -> Result[(), IoError | AllocError | FileInputError]:
    for line in fileinput.files([paths.from_text("data")?]?):
        drop(line?)
        return Ok(())
    Ok(())

def main() -> Result[(), AllocError | IoError | FileInputError | StreamError | BytesError]:
{body}
    Ok(())
"#
        ),
    )
    .unwrap();
    plenty::compile_file_to_executable(&source, &executable, None)
        .unwrap_or_else(|error| panic!("{error}"));
    executable
}

fn run(executable: &Path, args: &[&Path], input: &[u8]) -> Output {
    support::output_within(
        Command::new(executable)
            .args(args)
            .current_dir(executable.parent().unwrap()),
        input,
        std::time::Duration::from_secs(20),
    )
}

fn success(output: Output, expected: &[u8]) {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, expected);
}

const ECHO: &str = r#"    for line in fileinput.lines()?:
        write_stdout(line?)?
    flush_stdout()?"#;

#[test]
fn raw_argv_stdin_empty_files_dash_and_retained_endings() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(dir.path(), ECHO);
    success(
        run(&exe, &[], b"one\r\n\nlast\rpart"),
        b"one\r\n\nlast\rpart",
    );
    success(run(&exe, &[], b""), b"");
    let a = dir.path().join("a space");
    let empty = dir.path().join("empty");
    let b = dir.path().join("b");
    std::fs::write(&a, b"A\nend").unwrap();
    std::fs::write(&empty, b"").unwrap();
    std::fs::write(&b, b"B\r\n").unwrap();
    success(
        run(
            &exe,
            &[&empty, &a, Path::new("-"), &b, Path::new("-"), &empty],
            b"stdin\n",
        ),
        b"A\nendstdin\nB\r\n",
    );
    // Convenience does not parse application options.
    let output = run(&exe, &[Path::new("--unknown")], b"");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("FileInputError.Open"));
}

#[test]
fn explicit_paths_and_empty_list_ignore_application_arguments() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("chosen"), b"chosen\n").unwrap();
    let exe = compile(
        dir.path(),
        r#"    for line in fileinput.files([paths.from_text("chosen")?]?):
        write_stdout(line?)?
    empty: list[paths.Path] = []?
    for line in fileinput.files(empty):
        write_stdout(line?)?
    flush_stdout()?"#,
    );
    success(
        run(&exe, &[Path::new("--app-flag")], b"fallback"),
        b"chosen\nfallback",
    );
}

#[test]
fn large_lines_boundaries_and_split_utf8_on_files_and_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(dir.path(), ECHO);
    let mut data = vec![b'a'; 8191];
    data.extend_from_slice("€\r\n".as_bytes());
    data.extend(vec![b'b'; 2 * 1024 * 1024]);
    data.extend_from_slice(b"\n\nfinal");
    let path = dir.path().join("large");
    std::fs::write(&path, &data).unwrap();
    success(run(&exe, &[&path], b""), &data);
    success(run(&exe, &[], &data), &data);
}

#[cfg(unix)]
#[test]
fn native_paths_symlinks_and_open_path_modes() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let native = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"native \xff".to_vec()));
    std::fs::write(&native, b"native\n").unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&native, &link).unwrap();
    let exe = compile(dir.path(), ECHO);
    success(run(&exe, &[&native, &link], b""), b"native\nnative\n");
    let exe = compile(
        dir.path(),
        r#"    path = paths.from_text("modes")?
    mut file = io.open_path(&path, "w+")?
    io.write_all(&mut file, b"safe", 0, 4).unwrap()
    file.close()?
    print(io.open_path(&path, "x").is_err())?
    print(io.open_path(&path, "bad").is_err())?
    bad = paths.from_bytes([109u8, 111u8, 100u8, 101u8, 115u8, 0u8, 120u8]?)
    print(io.open_path(&bad, "w").is_err())?
    print(read_text("modes")?)?"#,
    );
    success(run(&exe, &[], b""), b"True\nTrue\nTrue\nsafe\n");
}

#[test]
fn open_read_decode_errors_yield_once_and_stop_before_later_paths() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(
        dir.path(),
        r#"    for line in fileinput.lines()?:
        match line:
            case Ok(text):
                write_stdout(text)?
            case Err(error):
                print(error)?
    print("done")?"#,
    );
    let bad = dir.path().join("bad");
    let later = dir.path().join("later");
    std::fs::write(&later, b"MUST NOT READ\n").unwrap();
    std::fs::write(&bad, b"good\n\xff\n").unwrap();
    for (path, variant, cause) in [
        (dir.path().join("missing"), "Open", "System(2)"),
        (dir.path().to_owned(), "Read", "System(21)"),
        (bad, "Text", "InvalidUtf8"),
    ] {
        let output = run(&exe, &[&path, &later], b"");
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.matches("FileInputError.").count(), 1, "{text}");
        assert!(
            text.contains(&format!("FileInputError.{variant}")),
            "{text}"
        );
        assert!(text.contains(cause), "{text}");
        assert!(!text.contains("MUST NOT READ"));
        assert!(text.ends_with("done\n"));
    }
    let exe = compile(dir.path(), ECHO);
    let output = run(&exe, &[], b"\xff");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("InvalidUtf8"));
}

#[test]
fn file_cleanup_on_exhaustion_break_error_and_propagation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data"), b"first\nsecond\n").unwrap();
    std::fs::write(dir.path().join("invalid"), b"\xff\n").unwrap();
    let exe = compile(
        dir.path(),
        r#"    before = descriptors()?
    for index in range(40):
        for line in fileinput.files([paths.from_text("data")?]?):
            drop(line?)
            break
        for line in fileinput.files([paths.from_text("data")?]?):
            drop(line?)
        mut failed = fileinput.files([paths.from_text("invalid")?]?)
        drop(next(&mut failed))
        print(descriptors()? == before)?
        drop(failed)
    drop(propagate_file_error())
    return_early()?
    print(descriptors()? == before)?
    for line in fileinput.files([]?):
        drop(line?)
        break
    mut stdin = io.stdin()
    mut buffer = buffers.empty()?
    buffer.resize(1, 0u8)?
    print(stdin.read(&mut buffer, 0, 1).is_ok())?"#,
    );
    success(run(&exe, &[], b"stdin\n"), "True\n".repeat(42).as_bytes());
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_a_terminal_error_and_drops_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("data"),
        b"first\nsecond long enough to allocate\n",
    )
    .unwrap();
    let exe = compile(
        dir.path(),
        r#"    before = descriptors()?
    mut source = fileinput.files([paths.from_text("data")?]?)
    drop(next(&mut source))
    print("__test_fail_allocations_after_0__")?
    outcome = next(&mut source)
    print("__test_restore_allocations__")?
    print(outcome)?
    print(next(&mut source).is_none(), descriptors()? == before)?"#,
    );
    let output = run(&exe, &[], b"");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Allocation(AllocError.OutOfMemory)"),
        "{text}"
    );
    assert!(text.ends_with("True True\n"), "{text}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn open_path_allocation_failure_does_not_truncate() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("safe"), b"preserved").unwrap();
    let exe = compile(
        dir.path(),
        r#"    path = paths.from_text("safe")?
    before = descriptors()?
    print("__test_fail_allocations_after_0__")?
    outcome = io.open_path(&path, "w")
    print("__test_restore_allocations__")?
    print(str.repr(outcome)?)?
    print(read_text("safe")?, descriptors()? == before)?"#,
    );
    let output = run(&exe, &[], b"");
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Allocation(AllocError.OutOfMemory)"),
        "{text}"
    );
    assert!(text.ends_with("preserved True\n"), "{text}");
}

#[test]
fn records_do_not_cross_files_or_split_on_lone_cr() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(
        dir.path(),
        r#"    for line in fileinput.lines()?:
        print(str.repr(line?)?)?"#,
    );
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, b"a\rb\nunterminated").unwrap();
    std::fs::write(&b, b"next\r\n\n").unwrap();
    success(
        run(&exe, &[&a, &b], b""),
        b"\"a\\rb\\n\"\n\"unterminated\"\n\"next\\r\\n\"\n\"\\n\"\n",
    );
}

#[test]
fn delimiters_at_and_across_buffer_boundaries_do_not_add_records() {
    let dir = tempfile::tempdir().unwrap();
    let exe = compile(
        dir.path(),
        r#"    for line in fileinput.lines()?:
        text = line?
        print(text.byte_len(), len(text), text.endswith("\n"), text.endswith("\r\n"))?"#,
    );
    for (tail, expected) in [
        ("\n", "8192 8192 True False\n"),
        ("\r\n", "8193 8193 True True\n"),
        ("€\n", "8195 8193 True False\n"),
        ("€", "8194 8192 False False\n"),
    ] {
        let mut data = vec![b'x'; 8191];
        data.extend_from_slice(tail.as_bytes());
        success(run(&exe, &[], &data), expected.as_bytes());
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn opening_and_buffering_failpoints_release_owned_descriptors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data"), b"a sufficiently long first line\n").unwrap();
    let exe = compile(
        dir.path(),
        r#"    before = descriptors()?
    for allowance in range(12):
        mut source = fileinput.files([paths.from_text("data")?]?)
        marker = (("__test_fail_allocations_after_" + str.from(allowance)?)? + "__")?
        print(marker)?
        outcome = next(&mut source)
        print("__test_restore_allocations__")?
        print(outcome.is_some())?
        drop(outcome)
        drop(source)
        print(descriptors()? == before)?"#,
    );
    let expected: String = (0..12)
        .map(|allowance| {
            format!("__test_fail_allocations_after_{allowance}__\n__test_restore_allocations__\nTrue\nTrue\n")
        })
        .collect();
    success(run(&exe, &[], b""), expected.as_bytes());
}

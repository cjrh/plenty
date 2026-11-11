//! `unchecked:` and `unchecked trap:` suites check their arithmetic and
//! indexing preconditions and trap on a violation (B14).
mod support;
use plenty::CompileOptions;
use std::path::Path;
use std::process::{Command, Output};
use support::plenty_bin;

fn options() -> CompileOptions {
    CompileOptions {
        cache: plenty::cache::CacheMode::Off,
        ..Default::default()
    }
}

fn run(dir: &Path, source: &str, options: &CompileOptions) -> Output {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable_with_options(&path, &executable, None, options)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    Command::new(executable).output().unwrap()
}

fn assert_traps(output: &Output, message: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(message), "{stderr}");
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

const OVERFLOW: &str = r#"
def add(a: i64, b: i64) -> i64:
    unchecked:
        return a + b
def main() -> Result[(), IoError]:
    print(add(i64.MAX, 1))?
    Ok(())
"#;

#[test]
fn plain_unchecked_traps_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), OVERFLOW, &options());
    assert_traps(&output, "integer overflow");
}

#[test]
fn unchecked_trap_traps() {
    let source = r#"
def add(a: i64, b: i64) -> i64:
    unchecked trap:
        return a + b
def main() -> Result[(), IoError]:
    print(add(i64.MAX, 1))?
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    assert_traps(&run(dir.path(), source, &options()), "integer overflow");
}

const KERNEL: &str = r#"
def kernel(a: i64, b: i64, s: i64, x: i64) -> i64:
    unchecked:
        return (a // b) + (a % b) + (a << s) + i64(i32(x)) + a * b - b + a.pow(2)
def main() -> Result[(), IoError]:
    print(kernel(7, -2, 3, -5))?
    unchecked:
        print(i64.MIN % -1, 1u64 << 63)?
    Ok(())
"#;

#[test]
fn valid_preconditions_keep_their_results() {
    // 7 // -2 = -4, 7 % -2 = -1, 7 << 3 = 56, -5, 7 * -2 = -14, + 2, 49.
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        stdout(&run(dir.path(), KERNEL, &options())),
        "83\n0 9223372036854775808\n"
    );
}

#[test]
fn nested_suites_select_their_own_contracts() {
    let source = r#"
def nested(a: i64, b: i64) -> i64:
    unchecked trap:
        x = a * b
        y = unchecked:
            a + b
        return x + y
def recoverable(a: i64) -> Result[i64, OverflowError[i64]]:
    unchecked:
        return checked:
            a + 1
def main() -> Result[(), IoError]:
    print(nested(3, 4), recoverable(i64.MAX).is_err())?
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), source, &options());
    assert_eq!(stdout(&output), "19 True\n");
}

#[test]
fn closures_keep_their_own_contracts() {
    let source = r#"
def apply(a: i64) -> i64:
    unchecked:
        f = def(x: i64) -> i64:
            return x + 1
        return f(a)
def main() -> ():
    pass
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let error = plenty::compile_file_to_executable_with_options(
        &path,
        &dir.path().join("program"),
        None,
        &options(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("OverflowError[i64]"), "{error}");
}

#[test]
fn tail_suite_checks_only_the_enclosing_block_for_unreachable_code() {
    let source = r#"
def f(a: i64, b: i64) -> i64:
    unchecked:
        x = a * b
        return x + 1
def main() -> Result[(), IoError]:
    print(f(3, 4))?
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), source, &options());
    assert_eq!(stdout(&output), "13\n");
}

#[test]
fn trap_stays_a_name() {
    let source = r#"
def main() -> Result[(), IoError]:
    trap = 2
    unchecked: i64 = trap
    unchecked trap:
        print(unchecked + trap)?
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), source, &options());
    assert_eq!(stdout(&output), "4\n");
}

#[test]
fn unchecked_is_not_a_command_line_option() {
    // The `--unchecked` build mode was removed; it is an unknown option.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, OVERFLOW).unwrap();
    let output = Command::new(plenty_bin())
        .args(["--unchecked", "assume"])
        .arg(&path)
        .env("PLENTY_CACHE", "off")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("unrecognised arguments"), "{stderr}");
    assert!(output.stdout.is_empty(), "{stderr}");
}

#[test]
fn unchecked_indexing_checks_bounds_and_counts_negative_indices_from_the_end() {
    let template = r#"
def read(xs: &list[i64], i: i64) -> i64:
    BODY
def main() -> Result[(), AllocError | IoError]:
    xs = [3, 4]?
    unchecked:
        print(read(&xs, INDEX))?
    Ok(())
"#;
    for body in [
        "return xs[i]",
        "unchecked:\n        return xs[i]",
        "unchecked trap:\n        return xs[i]",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let source = template.replace("BODY", body);
        assert_eq!(
            stdout(&run(dir.path(), &source.replace("INDEX", "-1"), &options())),
            "4\n"
        );
        for index in ["2", "-3"] {
            assert_traps(
                &run(dir.path(), &source.replace("INDEX", index), &options()),
                "index out of bounds",
            );
        }
    }
}

#[test]
fn unchecked_indexing_preserves_owned_temporaries_and_resized_lists() {
    let source = r#"
from std.bytes import from_bytes
@copy
class Pair:
    a: i64
    b: i64

def temporary(i: i64) -> Result[(), AllocError | IoError]:
    unchecked:
        value = [Pair(11, 12)]?[i]
        text = ["first", "last"]?[i]
        byte = from_bytes(b"xy")?[i]
        print(value.a, value.b, text, byte)?
    Ok(())
def main() -> Result[(), AllocError | IoError]:
    temporary(0)?
    mut xs = [1]?
    unchecked:
        for i in range(3):
            xs[0] += 1
            xs.append(i)?
            xs[i] = i
        print(xs)?
        r = &mut xs[0]
        *r = 42
    print(xs[0])?
    Ok(())
"#;
    for optimize in [false, true] {
        let mut opts = options();
        opts.optimize = optimize;
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            stdout(&run(dir.path(), source, &opts)),
            "11 12 first 120\n[2, 1, 2, 2]\n42\n"
        );
    }
}

#[test]
fn closures_checked_blocks_and_methods_in_unchecked_suites_check_indices() {
    for expression in [
        "f = def(values: &list[i64], i: i64) -> i64:\n            values[i]\n        print(f(&xs, 2))?",
        "r = checked:\n            xs[2] + 1",
        "xs.swap(0, 2)",
    ] {
        let source = format!(
            r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [3, 4]?
    unchecked:
        {expression}
    Ok(())
"#
        );
        let dir = tempfile::tempdir().unwrap();
        assert_traps(&run(dir.path(), &source, &options()), "index out of bounds");
    }
}

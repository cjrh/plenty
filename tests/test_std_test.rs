//! `std.test` assertions and fixtures, and the per-function test runner of
//! `plenty test`.
use std::path::Path;
use std::process::{Command, Output};

fn run_program(source: &str) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    Command::new(executable)
        .env("TMPDIR", dir.path())
        .output()
        .unwrap()
}

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("import std.test\n\ndef main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn passing_assertions_are_silent() {
    let output = run_program(&program(
        r#"
std.test.check(1 < 2, "ordered")?
std.test.equal(&3, &3)?
values = [1, 2]?
same = [1, 2]?
std.test.equal(&values, &same)?
std.test.equal(&"text", &"text")?
std.test.not_equal(&1u8, &2u8)?
std.test.less(&"a", &"b")?
std.test.close(0.1 + 0.2, 0.3, 0.000000001)?
good: Result[i64, IoError] = Ok(1)
std.test.is_ok(&good)?
bad: Result[i64, IoError] = Err(IoError.Closed)
std.test.is_err(&bad)?
done: Result[(), IoError] = Ok(())
std.test.is_ok(&done)?
closed: Result[(), IoError] = Err(IoError.Closed)
std.test.is_err(&closed)?
unit_error: Result[i64, ()] = Err(())
std.test.is_err(&unit_error)?
std.test.equal(&(), &())?
std.test.equal((), ())?
"#,
    ));
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty() && output.stdout.is_empty());
}

#[test]
fn failed_assertions_show_their_values() {
    let cases = [
        (
            "std.test.equal(&2 + 2, &5)?",
            "assertion failed: left == right\n  left: 4\n right: 5\n",
        ),
        (
            "left = [\"a\", \"b\"]?\nright = [\"a\"]?\nstd.test.equal(&left, &right)?",
            "assertion failed: left == right\n  left: [\"a\", \"b\"]\n right: [\"a\"]\n",
        ),
        (
            "std.test.not_equal(&\"x\", &\"x\")?",
            "assertion failed: left != right\n  both: \"x\"\n",
        ),
        (
            "std.test.less(&3, &3)?",
            "assertion failed: left < right\n  left: 3\n right: 3\n",
        ),
        (
            "std.test.check(False, \"cache is warm\")?",
            "assertion failed: cache is warm\n",
        ),
        (
            "std.test.fail(\"unreachable branch\")?",
            "assertion failed: unreachable branch\n",
        ),
        (
            "std.test.close(1.0, 1.5, 0.25)?",
            "assertion failed: |left - right| <= tolerance\n      left: 1.0\n     right: 1.5\n tolerance: 0.25\n",
        ),
        (
            "nan = 0.0 / 0.0\nstd.test.close(nan, nan, 1.0)?",
            "assertion failed: |left - right| <= tolerance\n      left: NaN\n     right: NaN\n tolerance: 1.0\n",
        ),
        (
            "result: Result[i64, IoError] = Err(IoError.Closed)\nstd.test.is_ok(&result)?",
            "assertion failed: result is Ok\n  error: IoError.Closed\n",
        ),
        (
            "result: Result[i64, IoError] = Ok(7)\nstd.test.is_err(&result)?",
            "assertion failed: result is Err\n  value: 7\n",
        ),
        (
            "result: Result[(), IoError] = Ok(())\nstd.test.is_err(&result)?",
            "assertion failed: result is Err\n  value: ()\n",
        ),
        (
            "result: Result[i64, ()] = Err(())\nstd.test.is_ok(&result)?",
            "assertion failed: result is Ok\n  error: ()\n",
        ),
        (
            "std.test.not_equal(&(), &())?",
            "assertion failed: left != right\n  both: ()\n",
        ),
    ];
    for (body, expected) in cases {
        let output = run_program(&program(body));
        assert_eq!(output.status.code(), Some(1), "{body}");
        // Each case asserts on its last line; `program` starts the body on line 4.
        let line = 3 + body.lines().count();
        let expected = format!("main.plenty:{line}:5: {expected}");
        let report = stderr(&output);
        assert!(
            report.starts_with(&expected),
            "{body}\nexpected {expected:?}\ngot {report:?}"
        );
        assert!(
            report.ends_with("error: main returned Failure.Unspecified: Failure keeps no details of the original error\n"),
            "{report}"
        );
    }
}

#[test]
fn output_before_a_failure_is_flushed_first() {
    let output = run_program(
        "import std.test\n\ndef main() -> Result[(), Failure]:\n    print(\"before\")?\n    std.test.equal(&1, &2)?\n    Ok(())\n",
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "before\n");
    assert!(stderr(&output).starts_with("main.plenty:5:5: assertion failed: left == right\n"));
}

#[test]
fn failures_name_the_user_code_that_asserted() {
    let output = run_program(
        r#"import std.test

def same_length(left: &str, right: &str) -> Result[(), Failure]:
    std.test.equal(&len(left), &len(right))?
    Ok(())

def main() -> Result[(), Failure]:
    same_length("ab", "ab")?
    same_length("ab", "abc")?
    Ok(())
"#,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with("main.plenty:4:5: assertion failed: left == right\n"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn caller_location_hooks_are_reserved_for_std() {
    for call in [
        "drop(__caller_location())",
        "drop(__test_fault(0, 0, 0i32))",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        std::fs::write(&path, format!("def main() -> ():\n    {call}\n")).unwrap();
        let error = plenty::compile_file_to_executable(&path, &dir.path().join("program"), None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("is reserved for the standard library"),
            "{error}"
        );
    }
}

#[test]
fn failure_injection_needs_a_test_build() {
    let output = run_program(&program("std.test.fail_allocations_after(0u32)?"));
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output)
            .starts_with("error: failure injection needs an executable built by `plenty test`\n"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn scratch_directories_are_removed() {
    let output = run_program(
        r#"import std.fs
import std.path
import std.test
from std.test import Scratch

def make() -> Result[str, Failure]:
    scratch = std.test.scratch()?
    write_text(scratch.file("dropped.txt")?, "x")?
    Ok(scratch.path().to_text()?)

def main() -> Result[(), Failure]:
    mut scratch = std.test.scratch()?
    std.test.check(scratch.path().is_absolute(), "absolute")?
    write_text(scratch.file("notes.txt")?, "hello")?
    std.test.equal(&read_text(scratch.file("notes.txt")?)?, &"hello")?
    scratch.root().create_dir(&std.path.from_text("nested")?)?
    write_text(scratch.file("nested")?.concat("/deep.txt")?, "deep")?
    kept = scratch.path().copy()?
    scratch.cleanup()?
    scratch.cleanup()?
    std.test.check(std.fs.metadata(&kept).is_err(), "removed by cleanup")?
    dropped = std.path.from_text(&make()?)?
    std.test.check(std.fs.metadata(&dropped).is_err(), "removed on drop")?
    other = std.test.scratch()?
    std.test.not_equal(&other.path().to_text()?, &kept.to_text()?)?
    Ok(())
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn scratch_uses_tmpdir() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(
        &path,
        "import std.test\n\ndef main() -> Result[(), Failure]:\n    scratch = std.test.scratch()?\n    print(scratch.path().to_text()?)?\n    Ok(())\n",
    )
    .unwrap();
    plenty::compile_file_to_executable(&path, &executable, None).unwrap();
    let base = dir.path().join("base");
    std::fs::create_dir(&base).unwrap();
    let output = Command::new(&executable)
        .env("TMPDIR", &base)
        .output()
        .unwrap();
    let printed = String::from_utf8(output.stdout).unwrap();
    let printed = Path::new(printed.trim_end());
    assert_eq!(printed.parent().unwrap(), base);
    assert!(printed
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("plenty-test-"));
    assert_eq!(std::fs::read_dir(&base).unwrap().count(), 0);
    // A relative TMPDIR is ignored in favour of /tmp.
    let output = Command::new(&executable)
        .env("TMPDIR", "relative")
        .output()
        .unwrap();
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with("/tmp/plenty-test-"));
}

#[test]
fn seeds_come_from_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(
        &path,
        "import std.random\nimport std.test\n\ndef main() -> Result[(), Failure]:\n    mut rng = std.test.rng()?\n    mut expected = std.random.seeded(std.test.seed()?)\n    print(std.test.seed()?, rng.next_u64() == expected.next_u64())?\n    Ok(())\n",
    )
    .unwrap();
    plenty::compile_file_to_executable(&path, &executable, None).unwrap();
    let run = |seed: Option<&str>| {
        let mut command = Command::new(&executable);
        command.env_remove("PLENTY_TEST_SEED");
        if let Some(seed) = seed {
            command.env("PLENTY_TEST_SEED", seed);
        }
        command.output().unwrap()
    };
    assert_eq!(run(None).stdout, b"0 True\n");
    assert_eq!(run(Some("42")).stdout, b"42 True\n");
    let invalid = run(Some("-1"));
    assert_eq!(invalid.status.code(), Some(1));
    assert!(stderr(&invalid).starts_with("PLENTY_TEST_SEED is not a decimal u64\n"));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn assertions_allocate_only_to_report_a_failure() {
    let output = run_program(&program(
        r#"
values = [1, 2, 3]?
same = [1, 2, 3]?
print("__test_begin_no_allocations__")?
std.test.equal(&values, &same)?
std.test.check(True, "free")?
std.test.close(1.0, 1.0, 0.0)?
print("__test_end_no_allocations__")?
"#,
    ));
    assert!(output.status.success(), "{}", stderr(&output));

    let output = run_program(
        "import std.test\n\ndef main() -> Result[(), Failure]:\n    values = [1, 2, 3]?\n    other = [9]?\n    print(\"__test_fail_allocations_after_0__\")?\n    std.test.equal(&values, &other)?\n    Ok(())\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with(
            "main.plenty:7:5: assertion failed: left == right\n  left: <no memory to show the value>\n"
        ),
        "{}",
        stderr(&output)
    );
}

// ---- plenty test ----

fn plenty(directory: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .args(args)
        .current_dir(directory)
        .env("TMPDIR", directory)
        .env_remove("PLENTY_TEST_SEED")
        .output()
        .unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn suite(root: &Path, files: &[(&str, &str)]) {
    write(
        &root.join("plenty.toml"),
        "manifest-version = 1\n[package]\nname = \"calc\"\nversion = \"0.1.0\"\n",
    );
    write(
        &root.join("src/lib.plenty"),
        "pub def add(a: i64, b: i64) -> i64:\n    a + b\n",
    );
    for (path, text) in files {
        write(&root.join(path), text);
    }
}

const ARITHMETIC: &str = r#"import std.test
from calc import add

def test_adds() -> Result[(), Failure]:
    std.test.equal(&add(2, 2), &4)?
    Ok(())

def test_wrong() -> Result[(), Failure]:
    print("about to compare")?
    std.test.equal(&add(2, 2), &5)?
    Ok(())

def helper() -> i64:
    1

def test_unit() -> ():
    pass

def test_typed() -> Result[(), std.test.TestError]:
    std.test.check(helper() == 1, "helper")

def test_trap() -> Result[(), Failure]:
    values = [1]?
    print(values[helper() + 4])?
    Ok(())
"#;

#[test]
fn test_functions_run_in_their_own_processes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    suite(
        root,
        &[
            ("tests/arithmetic.plenty", ARITHMETIC),
            (
                "tests/program.plenty",
                "def main() -> Result[(), Failure]:\n    print(\"program output\")?\n    Ok(())\n",
            ),
            (
                "tests/support/helpers.plenty",
                "pub def x() -> i64:\n    1\n",
            ),
        ],
    );
    let output = plenty(root, &["test"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout,
        "running 6 tests
test arithmetic::test_adds ... ok
test arithmetic::test_wrong ... FAILED (returned Err)
test arithmetic::test_unit ... ok
test arithmetic::test_typed ... ok
test arithmetic::test_trap ... FAILED (exit status 1)
test program ... ok

failures:

---- arithmetic::test_wrong output ----
about to compare
arithmetic.plenty:10:5: assertion failed: left == right
  left: 4
 right: 5

---- arithmetic::test_trap output ----
error: arithmetic.plenty:24:11: index out of bounds: index 5, length 1

test result: FAILED. 4 passed; 2 failed
"
    );
    // Passing tests' output is captured and discarded.
    assert!(!stdout.contains("program output"));

    let listed = plenty(root, &["test", "--list"]);
    assert_eq!(
        String::from_utf8_lossy(&listed.stdout),
        "arithmetic::test_adds\narithmetic::test_wrong\narithmetic::test_unit\narithmetic::test_typed\narithmetic::test_trap\nprogram\n"
    );

    let filtered = plenty(root, &["test", "add", "unit"]);
    assert!(filtered.status.success());
    assert_eq!(
        String::from_utf8_lossy(&filtered.stdout),
        "running 2 tests\ntest arithmetic::test_adds ... ok\ntest arithmetic::test_unit ... ok\n\ntest result: ok. 2 passed; 0 failed; 4 filtered out\n"
    );

    let uncaptured = plenty(root, &["test", "--no-capture", "program", "wrong"]);
    let stdout = String::from_utf8_lossy(&uncaptured.stdout);
    assert!(stdout.contains("about to compare\n") && stdout.contains("program output\n"));
    assert!(!stdout.contains("failures:"));
    assert!(String::from_utf8_lossy(&uncaptured.stderr).contains("assertion failed"));

    let none = plenty(root, &["test", "nothing-matches"]);
    assert!(none.status.success());
    assert_eq!(
        String::from_utf8_lossy(&none.stdout),
        "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 6 filtered out\n"
    );
    // Only files with selected tests are compiled.
    std::fs::remove_dir_all(root.join("target")).unwrap();
    assert!(plenty(root, &["test", "program"]).status.success());
    assert!(!root.join("target/debug/tests/arithmetic").exists());
}

#[test]
fn seeds_and_fixtures_repeat_under_the_runner() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    suite(
        root,
        &[(
            "tests/random.plenty",
            r#"import std.test

def test_draw() -> Result[(), Failure]:
    mut rng = std.test.rng()?
    print(std.test.seed()?, rng.next_u64())?
    std.test.fail("show output")?
    Ok(())

def test_scratch_left_by_failure() -> Result[(), Failure]:
    scratch = std.test.scratch()?
    write_text(scratch.file("data")?, "x")?
    std.test.fail("after writing")?
    Ok(())

def test_scratch_left_by_trap() -> Result[(), Failure]:
    scratch = std.test.scratch()?
    write_text(scratch.file("data")?, "x")?
    values = [1]?
    print(values[len(&values)])?
    Ok(())
"#,
        )],
    );
    let first = plenty(root, &["test", "--seed", "99"]);
    let second = plenty(root, &["test", "--seed", "99"]);
    assert_eq!(first.stdout, second.stdout);
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("\n99 "), "{stdout}");
    let default = String::from_utf8_lossy(&plenty(root, &["test"]).stdout).into_owned();
    assert!(default.contains("\n0 "), "{default}");
    // Every scratch directory was removed, including after a failure or a
    // trap, which skips destructors: the runner removes each test's TMPDIR.
    let leftovers: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("plenty-test-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    let output = plenty(root, &["test", "--seed", "x"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--seed \"x\" is not a u64"));
}

#[test]
fn discovery_errors_name_the_file() {
    let cases = [
        (
            "def main() -> i32:\n    0\n\ndef test_x() -> ():\n    pass\n",
            "both.plenty:1:1: a test file defines either `main` or `test_` functions, not both; move `test_x` to its own file",
        ),
        (
            "def helper() -> ():\n    pass\n",
            "both.plenty: a test file must define `main` or at least one `test_` function",
        ),
        (
            "def test_x(value: i64) -> ():\n    pass\n",
            "both.plenty:1:1: test function `test_x` must take no parameters and return `()` or `Result[(), E]`",
        ),
        (
            "def test_x() -> i64:\n    1\n",
            "test function `test_x` must take no parameters",
        ),
        (
            "def test_x[T]() -> ():\n    pass\n",
            "test function `test_x` must take no parameters",
        ),
        (
            "type Outcome = Result[(), Failure]\n\ndef test_x() -> Outcome:\n    Ok(())\n",
            "test function `test_x` must take no parameters",
        ),
        (
            "def test_x() -> ():\n    undefined()\n",
            "both.plenty:2:5",
        ),
    ];
    for (source, message) in cases {
        let dir = tempfile::tempdir().unwrap();
        suite(dir.path(), &[("tests/both.plenty", source)]);
        let output = plenty(dir.path(), &["test"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{source}");
        assert!(
            stderr.contains(message),
            "{source}\nexpected {message:?}\ngot {stderr}"
        );
    }
}

#[test]
fn a_relocated_toolchain_runs_tests_offline() {
    let dir = tempfile::tempdir().unwrap();
    let toolchain = dir.path().join("toolchain/bin/plenty");
    std::fs::create_dir_all(toolchain.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_plenty"), &toolchain).unwrap();
    let project = dir.path().join("project");
    suite(&project, &[("tests/arithmetic.plenty", ARITHMETIC)]);
    let output = Command::new(&toolchain)
        .args(["test", "adds"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("test arithmetic::test_adds ... ok"));
}

#[test]
fn returned_errors_are_named_only_when_they_say_something() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    suite(
        root,
        &[(
            "tests/errors.plenty",
            r#"import std.test

def test_explained() -> Result[(), std.test.TestError]:
    std.test.check(False, "explained")

def test_silent() -> Result[(), Failure]:
    Err(Failure.Unspecified)

def test_detailed() -> Result[(), IoError]:
    Err(IoError.Closed)
"#,
        )],
    );
    let output = plenty(root, &["test", "--jobs", "1"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "running 3 tests
test errors::test_explained ... FAILED (returned Err)
test errors::test_silent ... FAILED (returned Err)
test errors::test_detailed ... FAILED (returned Err)

failures:

---- errors::test_explained output ----
errors.plenty:4:5: assertion failed: explained

---- errors::test_detailed output ----
error: test_detailed returned IoError.Closed

test result: FAILED. 0 passed; 3 failed
"
    );
}

// Each test waits for a file the other creates, so they pass only when they
// run at the same time.
const RENDEZVOUS: &str = r#"import std.fs
import std.path
from std.time import milliseconds, seconds, sleep

def meet(mine: &str, theirs: &str) -> Result[(), Failure]:
    write_text(mine, "here")?
    other = std.path.from_text(theirs)?
    for _ in range(500):
        if std.fs.metadata(&other).is_ok():
            return Ok(())
        sleep(&milliseconds(10u32))
    Err(Failure.Unspecified)

def test_first() -> Result[(), Failure]:
    meet("first.flag", "second.flag")

def test_second() -> Result[(), Failure]:
    meet("second.flag", "first.flag")

def test_forever() -> ():
    sleep(&seconds(600u32))
"#;

#[test]
fn tests_run_in_parallel_and_time_out() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    suite(root, &[("tests/meet.plenty", RENDEZVOUS)]);
    let started = std::time::Instant::now();
    let output = plenty(root, &["test", "--jobs", "3", "--timeout", "1"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "running 3 tests
test meet::test_first ... ok
test meet::test_second ... ok
test meet::test_forever ... FAILED (timed out after 1s)

test result: FAILED. 2 passed; 1 failed
"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(60));

    // One at a time, the first test waits in vain until its timeout.
    for flag in ["first.flag", "second.flag"] {
        std::fs::remove_file(root.join(flag)).unwrap();
    }
    let output = plenty(
        root,
        &["test", "--jobs", "1", "--timeout", "1", "first", "second"],
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "running 2 tests
test meet::test_first ... FAILED (timed out after 1s)
test meet::test_second ... ok

test result: FAILED. 1 passed; 1 failed; 1 filtered out
"
    );

    for (args, message) in [
        (["--jobs", "0"], "--jobs \"0\" is not a positive count"),
        (
            ["--timeout", "0"],
            "--timeout \"0\" is not a positive number of seconds",
        ),
    ] {
        let output = plenty(root, &["test", args[0], args[1]]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    }
}

#[test]
fn fake_clocks_and_streams() {
    let output = run_program(
        r#"import std.bytes
import std.io
import std.test
import std.time
from std.io import StreamError
from std.time import Clock, Instant, nanoseconds, seconds

def waited[C: Clock](clock: &C, start: &Instant) -> u64:
    clock.now().saturating_duration_since(start).as_seconds()

def main() -> Result[(), Failure]:
    mut clock = std.test.fake_clock(10u64)
    start = clock.now()
    std.test.equal(&waited(&clock, &start), &0u64)?
    clock.advance(&seconds(5u32))?
    std.test.equal(&waited(&clock, &start), &5u64)?
    clock.set(u64.MAX)
    past_end = clock.advance(&nanoseconds(1u64))
    std.test.is_err(&past_end)?
    out_of_range: Result[(), RangeError] = Err(RangeError.OutOfRange)
    std.test.equal(&past_end, &out_of_range)?
    real = std.time.monotonic_clock()
    std.test.less(&waited(&real, &real.now()), &60u64)?

    mut stream = std.test.fake_stream(std.bytes.from_text("hello world")?)?
    stream.limit(4)
    mut buffer = std.bytes.with_capacity(16)?
    buffer.resize(16, 0u8)?
    std.test.equal(&stream.read(&mut buffer, 0, 16)?, &4)?
    stream.fail_after(0, StreamError.Interrupted)
    std.test.is_err(&stream.read(&mut buffer, 0, 16))?
    std.test.equal(&stream.read(&mut buffer, 0, 0)?, &0)?
    mut rest = std.bytes.empty()?
    std.test.equal(&std.io.read_up_to(&mut stream, &mut rest, 100)?, &7)?
    std.test.equal(&rest.to_text()?, &"o world")?
    std.test.is_err(&stream.read(&mut buffer, 4, 20))?

    stream.limit(0)
    message = std.bytes.from_text("abc")?
    stream.fail_after(1, StreamError.WriteZero)
    std.test.equal(&stream.write(&message, 0, 3)?, &3)?
    match stream.write(&message, 0, 3):
        case Err(StreamError.WriteZero):
            pass
        case _:
            std.test.fail("second write fails")?
    stream.limit(2)
    std.io.write_all(&mut stream, &message, 0, 3)?
    stream.flush()?
    std.test.equal(&stream.output.to_text()?, &"abcabc")?
    std.test.equal(&stream.calls(), &11)?
    print("done")?
    Ok(())
"#,
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.stdout, b"done\n");
}

const FAULTS: &str = r#"import std.test

def test_allocations() -> Result[(), Failure]:
    std.test.fail_allocations_after(0u32)?
    failed = [1, 2, 3]
    std.test.restore_allocations()?
    match failed:
        case Err(error):
            std.test.equal(&error, &AllocError.OutOfMemory)?
        case Ok(_):
            std.test.fail("allocation succeeded")?
    restored = [1, 2, 3]
    std.test.is_ok(&restored)?
    Ok(())

def test_files() -> Result[(), Failure]:
    scratch = std.test.scratch()?
    name = scratch.file("a.txt")?
    write_text(&name, "kept")?
    std.test.fail_files_after(0u32, 5i32)?
    failed = read_text(&name)
    std.test.restore_files()?
    match failed:
        case Err(error):
            std.test.equal(&error, &IoError.System(5i32))?
        case Ok(_):
            std.test.fail("read succeeded")?
    std.test.equal(&read_text(&name)?, &"kept")?
    Ok(())

def test_later_files() -> Result[(), Failure]:
    scratch = std.test.scratch()?
    name = scratch.file("b.txt")?
    # The open succeeds and creates the file; the write fails.
    std.test.fail_files_after(1u32, 28i32)?
    written = write_text(&name, "x")
    std.test.restore_files()?
    std.test.is_err(&written)?
    std.test.equal(&read_text(&name)?, &"")?
    Ok(())
"#;

#[test]
fn test_builds_inject_allocation_and_file_failures() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    suite(
        root,
        &[
            ("tests/faults.plenty", FAULTS),
            (
                "tests/program.plenty",
                "import std.test\n\ndef main() -> Result[(), Failure]:\n    std.test.fail_allocations_after(0u32)?\n    std.test.is_err(&[1])?\n    std.test.restore_allocations()?\n    Ok(())\n",
            ),
        ],
    );
    let output = plenty(root, &["test"]);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        stderr(&output)
    );
}

fn symbols(executable: &Path) -> String {
    let output = Command::new("nm").arg(executable).output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    String::from_utf8(output.stdout).unwrap()
}

/// Shipped programs never contain the failure-injection hooks; only the
/// executables `plenty test` builds do.
#[test]
fn only_test_builds_link_the_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    suite(root, &[("tests/faults.plenty", FAULTS)]);
    let source = root.join("main.plenty");
    write(
        &source,
        "import std.test\n\ndef main() -> Result[(), Failure]:\n    std.test.fail_allocations_after(0u32)?\n    Ok(())\n",
    );
    let shipped = root.join("shipped");
    plenty::compile_file_to_executable(&source, &shipped, None).unwrap();
    let shipped = symbols(&shipped);
    for hook in ["plenty_test_fault", "__wrap_malloc", "__wrap_read"] {
        assert!(!shipped.contains(hook), "{hook} in a shipped program");
    }

    let output = plenty(root, &["test"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let tested = symbols(&root.join("target/debug/tests/calc/faults"));
    for hook in ["plenty_test_fault", "__wrap_malloc", "__wrap_read"] {
        assert!(tested.contains(hook), "{hook} missing from a test build");
    }
}

use std::path::Path;

fn compile(dir: &Path, source: &str) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    executable
}

fn run(source: &str) -> String {
    let directory = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(compile(directory.path(), source))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn scalars_bounds_bases_width_and_escaping() {
    assert_eq!(run(r#"
import std.fmt
def main() -> Result[(), Failure]:
    mut out = std.fmt.string_formatter(500, 8)?
    out.text("é🙂")?
    out.integer(i64.MIN, 0)?
    out.text("/")?
    out.unsigned(u64.MAX, 16, 0)?
    out.text("/")?
    out.integer(-12, 6)?
    out.text("/")?
    out.unsigned(5u64, 2, 5)?
    out.text("/")?
    out.boolean(True)?
    out.text("/")?
    out.floating(1.25, 9)?
    out.text("/")?
    out.floating(-0.0, 0)?
    out.text("/")?
    out.debug_text("a\n\0\"\\é🙂\t\r")?
    print(std.fmt.finish(&out)?)?
    Ok(())
"#), "é🙂-9223372036854775808/ffffffffffffffff/   -12/  101/True/   1.25e0/-0e0/\"a\\n\\u{0}\\\"\\\\é🙂\\t\\r\"");
}

#[test]
fn user_methods_and_aggregate_callbacks_are_static() {
    assert_eq!(
        run(r#"
import std.fmt
from std.fmt import Formatter, FormatError
from std.io import Cursor
pub class Person:
    name: str
    pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.text(&self.name)
    pub def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        out.text("Person(")?
        out.debug_text(&self.name)?
        out.text(")")
def number(value: &i64, out: &mut Formatter[Cursor]) -> Result[(), FormatError]:
    out.integer(*value, 0)
def quoted(value: &str, out: &mut Formatter[Cursor]) -> Result[(), FormatError]:
    out.debug_text(value)
def main() -> Result[(), Failure]:
    person = Person("Amy")
    print(std.fmt.to_string(&person, 100, 8)?, std.fmt.to_debug(&person, 100, 8)?)?
    values = [1, -2, 3]?
    mut out = std.fmt.string_formatter(100, 8)?
    std.fmt.list_with(&values, &mut out, number)?
    pair = (42, "hello")
    std.fmt.pair_with(&pair, &mut out, number, quoted)?
    print(std.fmt.finish(&out)?)?
    Ok(())
"#),
        "Amy Person(\"Amy\")\n[1, -2, 3](42, \"hello\")"
    );
}

#[test]
fn short_interrupted_zero_invalid_and_failed_writers_keep_progress() {
    assert_eq!(run(r#"
import std.fmt
import std.bytes
from std.bytes import Bytes
from std.io import StreamError
pub class Sink:
    calls: i64
    fail: i64
    pub def write(self: &mut Sink, source: &Bytes, start: i64, stop: i64) -> Result[i64, StreamError]:
        self.calls += 1
        if self.calls == 1:
            return Err(StreamError.Interrupted)
        if self.calls == 4:
            if self.fail == 0:
                return Ok(0)
            if self.fail == 1:
                return Ok(stop - start + 1)
            if self.fail == 2:
                return Err(StreamError.OutOfRange)
        Ok(1)
    pub def flush(self: &mut Sink) -> Result[(), StreamError]:
        Ok(())
def main() -> Result[(), Failure]:
    for mode in range(4):
        mut scratch = std.bytes.with_capacity(32)?
        scratch.resize(32, 0u8)?
        mut out = std.fmt.formatter(Sink(0, mode), scratch, 100, 8)?
        match out.text("🙂"):
            case Ok(_):
                print("ok", out.completed())?
            case Err(error):
                print(error, out.completed())?
    Ok(())
"#), "std.fmt.FormatError.Output(std.io.StreamError.WriteZero) 2\nstd.fmt.FormatError.Output(std.io.StreamError.InvalidProgress) 2\nstd.fmt.FormatError.Output(std.io.StreamError.OutOfRange) 2\nok 4");
}

#[test]
fn size_spec_and_recursion_limits_are_recoverable() {
    assert_eq!(
        run(r#"
import std.fmt
from std.fmt import Formatter, FormatError
pub class Recursive:
    value: i64
    pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        std.fmt.display(self, out)
def failed[T](value: Result[T, FormatError]) -> bool:
    match value:
        case Ok(_):
            False
        case Err(_):
            True
def main() -> Result[(), Failure]:
    mut out = std.fmt.string_formatter(3, 2)?
    print(failed(out.unsigned(1u64, 3, 0)), failed(out.integer(1, -1)), failed(out.floating(1.0, -1)))?
    print(out.completed())?
    out.text("ab")?
    print(failed(out.text("é")), out.completed(), std.fmt.finish(&out)?)?
    recursive = Recursive(1)
    print(failed(std.fmt.display(&recursive, &mut out)))?
    out.enter()?
    out.leave()
    out.text("c")?
    print(std.fmt.finish(&out)?)?
    print(failed(std.fmt.string_formatter(-1, 8)))?
    Ok(())
"#),
        "True True True\n0\nTrue 2 ab\nTrue\nabc\nTrue"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn streaming_uses_no_allocations_and_string_failures_are_values() {
    assert_eq!(
        run(r#"
import std.fmt
import std.bytes
import std.io
def main() -> Result[(), Failure]:
    mut scratch = std.bytes.with_capacity(32)?
    scratch.resize(32, 0u8)?
    data = std.bytes.with_capacity(256)?
    mut out = std.fmt.formatter(std.io.cursor(data), scratch, 256, 8)?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    out.text("é🙂")?
    out.debug_text("a\n\0")?
    out.integer(i64.MIN, 22)?
    out.unsigned(u64.MAX, 16, 0)?
    out.floating(1.25, 8)?
    out.boolean(False)?
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(out.completed())?
    print("__test_fail_allocations_after_0__")?
    failure = std.fmt.finish(&out)
    print("__test_restore_allocations__")?
    match failure:
        case Ok(_):
            print("unexpected")?
        case Err(_):
            print("allocation failed")?
    print("__test_fail_allocations_after_0__")?
    construction = std.fmt.string_formatter(32, 8)
    invalid = std.fmt.string_formatter(-1, 8)
    print("__test_restore_allocations__")?
    match construction:
        case Ok(_):
            print("unexpected")?
        case Err(_):
            print("construction failed")?
    match invalid:
        case Err(error):
            match error:
                case std.fmt.FormatError.InvalidSpec:
                    print("invalid spec")?
                case _:
                    print("wrong error")?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#),
        "67\nallocation failed\nconstruction failed\ninvalid spec"
    );
}

#[test]
fn floats_cover_extremes_and_bytes_fill_preserves_tail() {
    assert_eq!(
        run(r#"
import std.fmt
import std.bytes
def main() -> Result[(), Failure]:
    mut out = std.fmt.string_formatter(256, 0)?
    out.floating(f64.INF, 0)?
    out.text(" ")?
    out.floating(-f64.INF, 0)?
    out.text(" ")?
    out.floating(f64.NAN, 0)?
    out.text(" ")?
    out.floating(1.7976931348623157e308, 0)?
    out.text(" ")?
    out.floating(5e-324, 0)?
    print(std.fmt.finish(&out)?)?
    mut bytes = std.bytes.with_capacity(40)?
    bytes.resize(40, 42u8)?
    print(bytes.write_float(1.25)?, bytes.len(), bytes.get(39)?)?
    bytes.truncate(31)?
    match bytes.write_float(1.0):
        case Ok(_):
            print("unexpected")?
        case Err(_):
            print("short")?
    Ok(())
"#),
        "inf -inf NaN 1.7976931348623157e308 5e-324\n6 40 42\nshort"
    );
}

#[test]
fn ordinary_visibility_ownership_and_contract_errors_remain_checked() {
    for (source, expected) in [
        ("import std.fmt\ndef main() -> Result[(), Failure]:\n    value = 42\n    drop(std.fmt.to_string(&value, 32, 8))\n    Ok(())\n", "display"),
        ("import std.fmt\ndef main() -> Result[(), Failure]:\n    mut out = std.fmt.string_formatter(32, 8)?\n    view = &out\n    out.text(\"changed\")?\n    print(view.completed())?\n    Ok(())\n", "conflicting borrow"),
        ("import std.fmt\ndef main() -> Result[(), Failure]:\n    out = std.fmt.string_formatter(32, 8)?\n    drop(copy(out))\n    Ok(())\n", "cannot be copied"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.plenty");
        std::fs::write(&path, source).unwrap();
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn relocated_compiler_has_formatting_without_repository_or_rust_tools() {
    let directory = tempfile::tempdir().unwrap();
    let compiler = directory.path().join("compiler");
    std::fs::copy(env!("CARGO_BIN_EXE_plenty"), &compiler).unwrap();
    let source = directory.path().join("main.plenty");
    std::fs::write(&source, "import std.fmt\ndef main() -> Result[(), Failure]:\n    mut out = std.fmt.string_formatter(32, 0)?\n    out.floating(1.25, 0)?\n    print(std.fmt.finish(&out)?)?\n    Ok(())\n").unwrap();
    let check = std::process::Command::new(&compiler)
        .arg("--check")
        .arg(&source)
        .env("PATH", "")
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let executable = directory.path().join("program");
    let built = std::process::Command::new(&compiler)
        .arg("--compile")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"1.25e0\n");
}

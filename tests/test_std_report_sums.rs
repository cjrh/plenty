//! The reporting protocol alone forwards through normalized anonymous sums.
use std::process::Command;

fn run(source: &str) -> (String, String) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("main.plenty");
    let binary = directory.path().join("program");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &binary, None)
        .unwrap_or_else(|error| panic!("{error}"));
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    (visible, String::from_utf8(output.stderr).unwrap())
}

fn rejects(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("main.plenty");
    std::fs::write(&file, source).unwrap();
    let error = plenty::check_file(&file, None).unwrap_err().to_string();
    assert!(error.contains(expected), "expected {expected:?}: {error}");
}

const TYPES: &str = r#"
import std.report
from std.report import Formatter, FormatError
pub class Label:
    text: str
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text(&self.text)
pub enum Code:
    Value(i64)
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        match self:
            case Code.Value(number):
                output.integer(*number)
"#;

#[test]
fn every_member_dispatches_through_generic_and_direct_calls() {
    let source = format!(
        r#"{TYPES}
from std.report import Formattable as Reporting
type Error = Code | Label
def generic[T: Reporting](value: &T, out: &mut Formatter) -> Result[(), FormatError]:
    value.format(out)
def report(value: &Error) -> Result[(), FormatError | IoError]:
    mut out = std.report.string_formatter(100, 1)?
    std.report.render(value, &mut out)?
    out.text("/")?
    generic(value, &mut out)?
    out.text("/")?
    value.format(&mut out)?
    print(std.report.finish(&out)?)?
    Ok(())
def main() -> Result[(), FormatError | IoError]:
    a: Error = Label("é🙂")
    b: Error = Code.Value(42)
    report(&a)?
    report(&b)?
    print(std.report.to_string(&a, 100, 1)?)?
    print(std.report.to_string(&b, 100, 1)?)?
    Ok(())
"#
    );
    assert_eq!(run(&source).0, "é🙂/é🙂/é🙂\n42/42/42\né🙂\n42");
}

#[test]
fn normalized_aliases_singletons_generic_members_and_temporaries() {
    let source = format!(
        r#"{TYPES}
pub class Wrapped[T]:
    value: T
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text("wrapped")
type Alias = Wrapped[i64]
type Nested = (Code | Alias) | (Alias | Code)
def choose() -> Nested:
    Wrapped[i64](1)
def main() -> Result[(), FormatError | IoError]:
    value: Nested = Wrapped[i64](9)
    other: Code | Wrapped[str] = Wrapped[str]("owned text")
    one: Alias | Alias = Wrapped[i64](2)
    print(std.report.to_string(&value, 100, 1)?)?
    print(std.report.to_string(&other, 100, 1)?)?
    print(std.report.to_string(&one, 100, 1)?)?
    mut out = std.report.string_formatter(100, 0)?
    choose().format(&mut out)?
    print(std.report.finish(&out)?)?
    Ok(())
"#
    );
    assert_eq!(run(&source).0, "wrapped\nwrapped\nwrapped\nwrapped");
}

#[test]
fn rejects_any_member_with_missing_wrong_or_private_method() {
    for (declaration, expected) in [
        ("pub class Bad:\n    value: i64\n", "missing method `format`"),
        ("pub class Bad:\n    value: i64\n    pub def format(self: &mut Bad, output: &mut Formatter) -> Result[(), FormatError]:\n        Ok(())\n", "signature of `format` does not match"),
        ("pub class Bad:\n    value: i64\n    pub def format(self, output: &Formatter) -> Result[(), FormatError]:\n        Ok(())\n", "signature of `format` does not match"),
        ("pub class Bad:\n    value: i64\n    pub def format(self, output: &mut Formatter) -> ():\n        pass\n", "signature of `format` does not match"),
        ("pub class Bad:\n    value: i64\n    pub def format[T](self, output: &mut Formatter) -> Result[(), FormatError]:\n        Ok(())\n", "signature of `format` does not match"),
        ("pub class Bad:\n    value: i64\n    def format(self, output: &mut Formatter) -> Result[(), FormatError]:\n        Ok(())\n", "private to its defining module"),
        ("class Bad:\n    value: i64\n    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:\n        Ok(())\n", "private to its defining module"),
    ] {
        for call in ["std.report.render(&value, &mut out)", "value.format(&mut out)"] {
            rejects(&format!(r#"{TYPES}
{declaration}
def main() -> Result[(), FormatError]:
    value: Label | Bad = Label("active valid member")
    mut out = std.report.string_formatter(100, 1)?
    {call}
"#), expected);
        }
    }
}

#[test]
fn user_protocol_does_not_gain_forwarding_after_report_dispatch_exists() {
    rejects(
        &format!(
            r#"{TYPES}
protocol Formattable:
    def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        pass
def custom[T: Formattable](value: &T, out: &mut Formatter) -> Result[(), FormatError]:
    value.format(out)
def main() -> Result[(), FormatError]:
    value: Label | Code = Code.Value(7)
    mut out = std.report.string_formatter(100, 1)?
    value.format(&mut out)?
    custom(&value, &mut out)
"#
        ),
        "anonymous sums only forward std.report.Formattable",
    );
}

#[test]
fn even_unused_local_bounds_require_public_forwarding_members() {
    rejects(
        &format!(
            r#"{TYPES}
from std.report import Formattable
pub class Private:
    value: i64
    def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        Ok(())
def accept[T: Formattable](value: &T) -> ():
    pass
def main() -> ():
    value: Code | Private = Code.Value(1)
    accept(&value)
"#
        ),
        "private to its defining module",
    );
}

#[test]
fn shared_dispatch_preserves_payload_and_cleans_up_once_after_failure() {
    let source = format!(
        r#"{TYPES}
pub class Owned:
    text: str
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text(&self.text)
    def __del__(self) -> ():
        print("drop", self.text).unwrap()
def main() -> Result[(), FormatError | IoError]:
    value: Owned | Code = Owned("payload")
    mut small = std.report.string_formatter(1, 1)?
    match std.report.render(&value, &mut small):
        case Err(_):
            print("failed")?
        case Ok(_):
            print("unexpected")?
    print(std.report.to_string(&value, 20, 1)?)?
    print(std.report.to_string(&value, 20, 1)?)?
    Ok(())
"#
    );
    assert_eq!(run(&source).0, "failed\npayload\npayload\ndrop payload");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn dispatch_and_stderr_reporting_do_not_allocate() {
    let source = format!(
        r#"{TYPES}
def main() -> Result[(), FormatError | IoError]:
    a: Label | Code = Label("text")
    b: Label | Code = Code.Value(-42)
    print("__test_fail_allocations_after_0__")?
    mut out = std.report.stderr_formatter(100, 1)?
    std.report.render(&a, &mut out)?
    std.report.render(&b, &mut out)?
    a.format(&mut out)?
    b.format(&mut out)?
    print("__test_restore_allocations__")?
    print(out.completed())?
    Ok(())
"#
    );
    assert_eq!(run(&source), ("14".into(), "text-42text-42".into()));
}

#[test]
fn inactive_member_effects_are_included_in_worker_validation() {
    rejects(
        &format!(
            r#"{TYPES}
import std.env
import std.path
pub class Global:
    value: i64
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        std.env.set_current_dir(&std.path.from_text(".").unwrap()).unwrap()
        Ok(())
def worker(value: &(Code | Global)) -> ():
    mut out = std.report.stderr_formatter(100, 1).unwrap()
    value.format(&mut out).unwrap()
def main() -> Result[(), Failure]:
    value: Code | Global = Code.Value(1)
    with spawn(worker, &value)?:
        pass
    Ok(())
"#
        ),
        "changing the working directory affects every thread",
    );
}

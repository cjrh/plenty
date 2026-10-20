//! `std.error`: contextual errors around typed causes, and their reports.
use std::process::Command;

/// Run a complete program as a compiled executable and through `plenty FILE`.
fn run_both(source: &str, code: i32, stdout: &str, stderr: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("entry.plenty");
    let executable = workspace.path().join("entry");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let mut through_cli = Command::new(env!("CARGO_BIN_EXE_plenty"));
    through_cli.arg(&file);
    for mut command in [Command::new(&executable), through_cli] {
        let output = command.current_dir(workspace.path()).output().unwrap();
        let visible: String = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(String::from_utf8_lossy(&output.stderr), stderr, "{source}");
        assert_eq!(visible, stdout, "{source}");
        assert_eq!(output.status.code(), Some(code), "{source}");
    }
}

const LOAD: &str = r#"from std.error import Context, Subject, context, path_context, native_code
from std.path import Path, from_bytes, from_text

def load(path: &Path) -> Result[list[u8], Context[IoError]]:
    path_context(read_bytes(path.as_bytes()), "read_bytes", path)

def start(path: &Path) -> Result[i64, Context[Context[IoError]]]:
    Ok(len(context(load(path), "load settings")?))
"#;

const FORMATTABLE_CAUSE: &str = r#"from std.error import Context, Subject
from std.report import Formatter, FormatError, render, to_string, string_formatter, stderr_formatter, finish
from std.path import from_text
pub enum Cause:
    Rejected
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text("rejected")
"#;

#[test]
fn typed_context_protocol_requires_its_entire_cause_to_be_formattable() {
    for cause in [
        "IoError.Closed",
        "Context(\"inner\", Subject.Absent, IoError.Closed)",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("main.plenty");
        std::fs::write(
            &file,
            format!(
                r#"{FORMATTABLE_CAUSE}
from std.report import Formattable
def accept[T: Formattable](value: &T) -> ():
    pass
def main() -> ():
    error = Context("outer", Subject.Absent, {cause})
    accept(&error)
"#
            ),
        )
        .unwrap();
        let error = plenty::check_file(&file, None).unwrap_err().to_string();
        assert!(error.contains("does not satisfy"), "{error}");
    }
}

#[test]
fn typed_context_formatting_nests_without_duplicating_the_cause() {
    run_both(
        &format!(
            r#"{FORMATTABLE_CAUSE}
def main() -> Result[(), AllocError | FormatError | IoError]:
    inner = Context("read", Subject.At(from_text("settings.txt")?), Cause.Rejected)
    outer = Context("start", Subject.Absent, inner)
    print(to_string(&outer, 100, 3)?)?
    print(outer.cause.cause == Cause.Rejected)?
    print(outer.cause.describe_with("custom description")?)?
    unavailable = Context("load", Subject.Unavailable(AllocError.OutOfMemory), Cause.Rejected)
    print(to_string(&unavailable, 100, 2)?)?
    Ok(())
"#
        ),
        0,
        "start: read \"settings.txt\": rejected\nTrue\nread \"settings.txt\": custom description\nload <path unavailable>: rejected\n",
        "",
    );
}

#[test]
fn typed_context_formatting_preserves_partial_output_and_depth() {
    run_both(
        &format!(
            r#"{FORMATTABLE_CAUSE}
def main() -> Result[(), AllocError | FormatError | IoError]:
    error = Context("open", Subject.Absent, Cause.Rejected)
    mut shallow = string_formatter(100, 1)?
    print(str.repr(render(&error, &mut shallow))?)?
    print(finish(&shallow)?)?
    render(&Cause.Rejected, &mut shallow)?
    print(finish(&shallow)?)?
    mut short = string_formatter(5, 2)?
    print(str.repr(render(&error, &mut short))?)?
    print(finish(&short)?)?
    print(error.cause == Cause.Rejected)?
    Ok(())
"#
        ),
        0,
        "Result[(), std.report.FormatError].Err(std.report.FormatError.RecursionLimit)\nopen: \nopen: rejected\nResult[(), std.report.FormatError].Err(std.report.FormatError.LimitExceeded)\nopen\nTrue\n",
        "",
    );
}

#[test]
fn typed_context_keeps_owned_cause_after_formatter_failure() {
    run_both(
        &format!(
            r#"{FORMATTABLE_CAUSE}
pub class Owned:
    pub text: str
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text(&self.text)?
        Err(FormatError.InvalidSpec)
    def __del__(self) -> ():
        print("drop", self.text).unwrap()
def main() -> Result[(), AllocError | FormatError | IoError]:
    error = Context("load", Subject.Absent, Owned("payload"))
    mut output = string_formatter(100, 2)?
    print(str.repr(render(&error, &mut output))?)?
    print(finish(&output)?)?
    print(error.cause.text)?
    Ok(())
"#
        ),
        0,
        "Result[(), std.report.FormatError].Err(std.report.FormatError.InvalidSpec)\nload: payload\npayload\ndrop payload\n",
        "",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn typed_context_without_a_path_formats_without_allocating() {
    run_both(
        &format!(
            r#"{FORMATTABLE_CAUSE}
def main() -> Result[(), AllocError | FormatError | IoError]:
    error = Context("read", Subject.Absent, Cause.Rejected)
    mut output = stderr_formatter(100, 2)?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    render(&error, &mut output)?
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(error.cause == Cause.Rejected)?
    Ok(())
"#
        ),
        0,
        "True\n",
        "read: rejected",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn typed_context_path_display_failure_preserves_its_cause() {
    run_both(
        &format!(
            r#"{FORMATTABLE_CAUSE}
def main() -> Result[(), AllocError | FormatError | IoError]:
    path = from_text("a/long/enough/path/to/require/an/allocation")?
    error = Context("read", Subject.At(path), Cause.Rejected)
    mut output = stderr_formatter(100, 2)?
    print("__test_fail_allocations_after_0__")?
    result = render(&error, &mut output)
    print("__test_restore_allocations__")?
    print(str.repr(result)?)?
    print(error.cause == Cause.Rejected)?
    print(output.completed())?
    Ok(())
"#
        ),
        0,
        "Result[(), std.report.FormatError].Err(std.report.FormatError.Bytes(std.bytes.BytesError.Allocation(AllocError.OutOfMemory)))\nTrue\n6\n",
        "read \"",
    );
}

#[test]
fn context_helpers_preserve_unit_success_and_typed_errors() {
    run_both(
        r#"from std.error import Context, context, path_context
from std.path import from_text

def flush() -> Result[(), Context[IoError]]:
    context(flush_stdout(), "flush")

def fail() -> Result[(), IoError]:
    Err(IoError.Closed)

def main() -> Result[(), AllocError | Context[IoError] | IoError]:
    path = from_text("output.txt")?
    flush()?
    path_context(flush_stdout(), "flush path", &path)?
    print("flushed")?
    match context(fail(), "close"):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error.describe()?)?
    match path_context(fail(), "close path", &path):
        case Ok(_):
            print("unexpected success")?
        case Err(error):
            print(error.describe()?)?
    Ok(())
"#,
        0,
        "flushed\nclose: IoError.Closed\nclose path \"output.txt\": IoError.Closed\n",
        "",
    );
}

#[test]
fn a_context_keeps_its_operation_path_and_typed_cause() {
    run_both(
        &format!(
            r#"{LOAD}
def main() -> Result[(), AllocError | IoError]:
    path = from_text("missing/settings")?
    match load(&path):
        case Ok(_):
            print("loaded")?
        case Err(error):
            print(error.operation)?
            match &error.subject:
                case Subject.At(kept):
                    print(kept.to_text()?)?
                case _:
                    print("no path")?
            print(native_code(error.cause))?
            print(error.cause == IoError.System(2i32))?
            print(error.describe()?)?
    print(native_code(IoError.Closed))?
    Ok(())
"#
        ),
        0,
        "read_bytes\nmissing/settings\nOption[i32].Some(2)\nTrue\nread_bytes \"missing/settings\": IoError.System(2)\nOption[i32].Nothing\n",
        "",
    );
}

#[test]
fn nested_contexts_keep_every_typed_cause_and_describe_outermost_first() {
    run_both(
        &format!(
            r#"{LOAD}
enum AppError:
    Settings(Context[Context[IoError]])
    Count(Context[ParseError])

def count(text: &str) -> Result[i64, AppError]:
    match context(i64.parse(text), "parse count"):
        case Ok(value):
            Ok(value)
        case Err(error):
            Err(AppError.Count(error))

def settings(path: &Path) -> Result[i64, AppError]:
    match start(path):
        case Ok(value):
            Ok(value)
        case Err(error):
            Err(AppError.Settings(error))

def show(error: &AppError) -> Result[(), AllocError | IoError]:
    match error:
        case AppError.Settings(outer):
            print(native_code(outer.cause.cause))?
            print(outer.describe_with(&outer.cause.describe()?)?)?
        case AppError.Count(inner):
            print(inner.describe()?)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    path = from_text("missing/settings")?
    match settings(&path):
        case Ok(_):
            print("loaded")?
        case Err(error):
            show(&error)?
    match count("many"):
        case Ok(_):
            print("counted")?
        case Err(error):
            show(&error)?
    print(count("12").unwrap())?
    Ok(())
"#
        ),
        0,
        "Option[i32].Some(2)\nload settings: read_bytes \"missing/settings\": IoError.System(2)\nparse count: ParseError.Invalid\n12\n",
        "",
    );
}

#[test]
fn paths_are_displayed_escaped_and_owned_causes_are_only_observed() {
    run_both(
        &format!(
            r#"{LOAD}
enum Rejected:
    Line(str)

def main() -> Result[(), AllocError | IoError]:
    path = from_bytes([109u8, 255u8, 10u8, 34u8, 92u8, 32u8, 122u8]?)
    match load(&path):
        case Ok(_):
            print("loaded")?
        case Err(error):
            print(error.describe()?)?
    owned = Context("check", Subject.Absent, Rejected.Line("a \"quoted\" line".concat("!")?))
    print(owned.describe()?)?
    print(owned.describe()?)?
    match owned.cause:
        case Rejected.Line(text):
            print(text)?
    moved = Context("open", Subject.At(path), IoError.NotReadable)
    print(moved.describe()?)?
    Ok(())
"#
        ),
        0,
        "read_bytes \"m\\xff\\x0a\\\"\\\\ z\": IoError.System(2)\ncheck: Rejected.Line(\"a \\\"quoted\\\" line!\")\ncheck: Rejected.Line(\"a \\\"quoted\\\" line!\")\na \"quoted\" line!\nopen \"m\\xff\\x0a\\\"\\\\ z\": IoError.NotReadable\n",
        "",
    );
}

#[test]
fn report_flushes_stdout_writes_one_stderr_line_and_leaves_the_status_to_main() {
    let source = format!(
        r#"{LOAD}
def main() -> i32:
    drop(write_stdout("before "))
    path = from_bytes([109u8, 255u8].unwrap())
    match start(&path):
        case Ok(_):
            0i32
        case Err(error):
            error.cause.report()
            error.report_with(error.describe_with(&error.cause.describe().unwrap()))
            print("after").unwrap()
            3i32
"#
    );
    run_both(
        &source,
        3,
        "before after\n",
        "error: read_bytes \"m\\xff\": IoError.System(2)\nerror: load settings: read_bytes \"m\\xff\": IoError.System(2)\n",
    );
    // A shared pipe shows the order: the report follows output already written.
    #[cfg(unix)]
    {
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("entry.plenty");
        std::fs::write(&file, &source).unwrap();
        let output = Command::new("sh")
            .arg("-c")
            .arg("exec \"$@\" 2>&1")
            .arg("sh")
            .arg(env!("CARGO_BIN_EXE_plenty"))
            .arg(&file)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("before error: read_bytes"));
    }
}

#[cfg(unix)]
#[test]
fn report_survives_a_closed_stderr() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("entry.plenty");
    std::fs::write(
        &file,
        "from std.error import Context, Subject\ndef main() -> i32:\n    Context(\"open\", Subject.Absent, IoError.Closed).report()\n    print(\"after\").unwrap()\n    4i32\n",
    )
    .unwrap();
    let output = Command::new("sh")
        .arg("-c")
        .arg("exec \"$@\" 2>&-")
        .arg("sh")
        .arg(env!("CARGO_BIN_EXE_plenty"))
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(output.stdout, b"after\n");
}

#[test]
fn entrypoint_reports_and_failure_erasure_are_unchanged() {
    run_both(
        &format!(
            r#"{LOAD}
def main() -> Result[(), Context[IoError]]:
    path = from_bytes([109u8].unwrap())
    load(&path)?
    Ok(())
"#
        ),
        1,
        "",
        "error: main returned std.error.Context[IoError](operation=\"read_bytes\", subject=std.error.Subject.At(std.path.Path(_bytes=[109])), cause=IoError.System(2))\n",
    );
    run_both(
        &format!(
            r#"{LOAD}
# B71: deliberately erases to Failure.
def erased() -> Result[i64, Context[Failure]]:
    failed: Result[i64, Failure] = Err(Failure.Unspecified)
    context(failed, "erased")

def main() -> Result[(), Failure]:
    match erased():
        case Ok(_):
            print("ok")?
        case Err(error):
            print(error.describe()?)?
    path = from_bytes([109u8]?)
    load(&path)?
    Ok(())
"#
        ),
        1,
        "erased: Failure.Unspecified\n",
        "error: main returned Failure.Unspecified: Failure keeps no details of the original error\n",
    );
}

#[test]
fn conversion_to_a_context_is_never_implicit() {
    for (body, diagnostic) in [
        (
            "def load() -> Result[str, Context[IoError]]:\n    Ok(read_text(\"a\")?)\n",
            "`?` requires identical Result error types; convert the error explicitly",
        ),
        (
            "def load() -> Result[str, IoError]:\n    Ok(context(read_text(\"a\"), \"read\")?)\n",
            "`?` requires identical Result error types; convert the error explicitly",
        ),
    ] {
        let source =
            format!("from std.error import Context, context\n{body}def main() -> ():\n    pass\n");
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("entry.plenty");
        std::fs::write(&file, &source).unwrap();
        let error = plenty::check_file(&file, None).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
    }
}

#[test]
fn a_cause_without_automatic_formatting_is_rejected_at_the_first_use() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("entry.plenty");
    std::fs::write(
        &file,
        "from std.error import Context, Subject\nenum Chain:\n    End\n    Link(Box[Chain])\ndef main() -> ():\n    error = Context(\"walk\", Subject.Absent, Chain.End)\n",
    )
    .unwrap();
    let error = plenty::check_file(&file, None).unwrap_err().to_string();
    assert!(
        error.contains("cannot instantiate `std.error.Context[Chain]`")
            && error.contains("automatic formatting is not supported for recursive data"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn a_failed_path_copy_keeps_the_cause_and_success_allocates_nothing() {
    run_both(
        &format!(
            r#"{LOAD}
def main() -> Result[(), AllocError | IoError]:
    path = from_text("a/long/enough/path/to/need/heap/storage")?
    print("__test_fail_allocations_after_0__")?
    failed = path_context(i64.parse("many"), "parse count", &path)
    print("__test_begin_no_allocations__")?
    kept = path_context(i64.parse("12"), "parse", &path)
    plain = context(i64.parse("many"), "parse")
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    match failed:
        case Ok(_):
            print("loaded")?
        case Err(error):
            match &error.subject:
                case Subject.Unavailable(reason):
                    print(reason)?
                case _:
                    print("unexpected subject")?
            print(error.cause == ParseError.Invalid)?
            print(error.describe()?)?
    print(kept.unwrap())?
    match plain:
        case Ok(_):
            print("parsed")?
        case Err(error):
            print(error.describe()?)?
    Ok(())
"#
        ),
        0,
        "AllocError.OutOfMemory\nTrue\nparse count <path unavailable>: ParseError.Invalid\n12\nparse: ParseError.Invalid\n",
        "",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reports_without_memory_use_the_nonallocating_fallbacks() {
    run_both(
        &format!(
            r#"{LOAD}
def main() -> i32:
    path = from_text("a/long/enough/path/to/need/heap/storage").unwrap()
    error = Context("read_bytes", Subject.At(path), IoError.System(2i32))
    ready = error.describe()
    print("__test_fail_allocations_after_0__").unwrap()
    described = error.describe()
    error.report()
    error.report_with(ready)
    print("__test_restore_allocations__").unwrap()
    print(str.repr(described).unwrap()).unwrap()
    error.report()
    2i32
"#
        ),
        2,
        "Result[str, AllocError].Err(AllocError.OutOfMemory)\n",
        "error: read_bytes: no memory to describe the error\nerror: read_bytes \"a/long/enough/path/to/need/heap/storage\": IoError.System(2)\nerror: read_bytes \"a/long/enough/path/to/need/heap/storage\": IoError.System(2)\n",
    );
}

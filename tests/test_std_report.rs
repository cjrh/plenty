use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod support;

fn compile(dir: &Path, source: &str) -> PathBuf {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{error}"));
    executable
}

fn run(source: &str) -> Output {
    let directory = tempfile::tempdir().unwrap();
    let result = Command::new(compile(directory.path(), source))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}

fn stdout(result: &Output) -> String {
    String::from_utf8_lossy(&result.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn unicode_nul_and_scalar_extremes() {
    let output = run(r#"
import std.report
from std.report import FormatError
def main() -> Result[(), FormatError | IoError]:
    mut out = std.report.string_formatter(200, 0)?
    out.text("é🙂\0")?
    out.integer(i64.MIN)?
    out.text("/")?
    out.integer(i64.MAX)?
    out.text("/")?
    out.unsigned(u64.MAX, 16)?
    out.text("/")?
    out.unsigned(9u64, 8)?
    out.text("/")?
    out.unsigned(5u64, 2)?
    out.text("/")?
    out.unsigned(0u64, 10)?
    out.boolean(True)?
    out.boolean(False)?
    print(std.report.finish(&out)?)?
    print(out.completed(), out.failed())?
    Ok(())
"#);
    let expected =
        "é🙂\0-9223372036854775808/9223372036854775807/ffffffffffffffff/11/101/0TrueFalse";
    assert_eq!(
        stdout(&output),
        format!("{expected}\n{} False", expected.len())
    );
}

#[test]
fn borrowed_class_enum_protocol_and_depth_cleanup() {
    assert_eq!(
        stdout(&run(r#"
import std.report
from std.report import Formatter, FormatError
pub class Label:
    value: str
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text(&self.value)
pub enum Message:
    Value(Label)
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        match self:
            case Message.Value(label):
                output.text("value=")?
                std.report.render(label, output)
def main() -> Result[(), FormatError | IoError]:
    value = Message.Value(Label("é"))
    print(std.report.to_string(&value, 20, 2)?)?
    print(std.report.to_string(&value, 20, 2)?)?
    mut out = std.report.string_formatter(20, 1)?
    match std.report.render(&value, &mut out):
        case Err(error):
            print(error, out.completed(), out.failed())?
        case Ok(_):
            print("unexpected")?
    std.report.render(&Label("ok"), &mut out)?
    print(std.report.finish(&out)?)?
    Ok(())
"#)),
        "value=é\nvalue=é\nstd.report.FormatError.RecursionLimit 6 False\nvalue=ok"
    );
}

#[test]
fn budgets_and_validation_preserve_prefix() {
    assert_eq!(
        stdout(&run(r#"
import std.report
from std.report import FormatError
def error[T](result: Result[T, FormatError]) -> FormatError:
    match result:
        case Err(value):
            value
        case Ok(_):
            FormatError.OutputClosed
def main() -> Result[(), FormatError | IoError]:
    mut out = std.report.string_formatter(4, 0)?
    out.text("a")?
    print(error(out.text("é🙂")), out.completed(), out.failed())?
    out.text("é")?
    print(error(out.integer(42)), out.completed())?
    print(std.report.finish(&out)?)?
    print(error(out.unsigned(1u64, 3)), out.completed())?
    print(error(out.enter()))?
    mut err = std.report.stderr_formatter(0, 64)?
    err.text("")?
    print(error(std.report.finish(&err)))?
    for index in range(2):
        depth = index * 66 - 1
        match std.report.stderr_formatter(1, depth):
            case Err(error):
                print(error)?
            case Ok(_):
                print("unexpected")?
    match std.report.string_formatter(-1, 1):
        case Err(error):
            print(error)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#)),
        "std.report.FormatError.LimitExceeded 1 False\nstd.report.FormatError.LimitExceeded 4\naé4\nstd.report.FormatError.InvalidSpec 4\nstd.report.FormatError.RecursionLimit\nstd.report.FormatError.InvalidSpec\nstd.report.FormatError.InvalidSpec\nstd.report.FormatError.InvalidSpec\nstd.report.FormatError.InvalidSpec"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn stderr_constructs_and_renders_without_allocations() {
    let output = run(r#"
import std.report
from std.report import Formatter, FormatError
pub enum Error:
    Code(i64)
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        match self:
            case Error.Code(code):
                output.text("é🙂\0code=")?
                output.integer(*code)?
                output.unsigned(u64.MAX, 16)?
                output.boolean(False)
def main() -> Result[(), FormatError | IoError]:
    value = Error.Code(i64.MIN)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    mut out = std.report.stderr_formatter(200, 2)?
    std.report.render(&value, &mut out)?
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(out.completed(), out.failed())?
    Ok(())
"#);
    let text = "é🙂\0code=-9223372036854775808ffffffffffffffffFalse";
    assert_eq!(output.stderr, text.as_bytes());
    assert_eq!(stdout(&output), format!("{} False", text.len()));
}

#[cfg(feature = "runtime-checks")]
#[test]
fn collector_allocation_failure_retains_valid_utf8_and_can_resume() {
    assert_eq!(
        stdout(&run(r#"
import std.report
from std.report import FormatError
def main() -> Result[(), FormatError | IoError]:
    mut out = std.report.string_formatter(100, 1)?
    out.text("éééé")?
    print("__test_fail_allocations_after_0__")?
    failure = out.text("🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂")
    finished = std.report.finish(&out)
    construction = std.report.string_formatter(100, 1)
    print("__test_restore_allocations__")?
    match failure:
        case Err(_):
            print("growth failed", out.completed(), out.failed())?
        case Ok(_):
            print("unexpected")?
    match finished:
        case Err(_):
            print("finish failed")?
        case Ok(_):
            print("unexpected")?
    match construction:
        case Err(_):
            print("construction failed")?
        case Ok(_):
            print("unexpected")?
    out.text("🙂")?
    print(std.report.finish(&out)?)?
    Ok(())
"#)),
        "growth failed 8 False\nfinish failed\nconstruction failed\néééé🙂"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn closed_stderr_is_sticky_and_still_drops_payload() {
    let directory = tempfile::tempdir().unwrap();
    let executable = compile(
        directory.path(),
        r#"
import std.report
from std.report import FormatError
class Resource:
    value: i64
    def __del__(self) -> ():
        print("dropped").unwrap()
def main() -> Result[(), FormatError | IoError]:
    resource = Resource(1)
    mut out = std.report.stderr_formatter(100, 1)?
    match out.text("hello"):
        case Err(_):
            print("failed", out.completed(), out.failed())?
        case Ok(_):
            print("unexpected")?
    match out.text("again"):
        case Err(error):
            print(error, out.completed())?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#,
    );
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let output = Command::new(executable).stderr(writer).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        "failed 0 True\nstd.report.FormatError.OutputClosed 0\ndropped"
    );
}

#[test]
fn protocol_rejects_missing_and_wrong_receiver() {
    for method in [
        "",
        "    pub def format(self: &mut Item, output: &mut Formatter) -> Result[(), FormatError]:\n        output.text(\"item\")\n",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let source = format!(
            "import std.report\nfrom std.report import Formatter, FormatError\npub class Item:\n    number: i64\n{method}def main() -> Result[(), FormatError]:\n    item = Item(1)\n    drop(std.report.to_string(&item, 20, 1))\n    Ok(())\n"
        );
        let path = directory.path().join("main.plenty");
        std::fs::write(&path, source).unwrap();
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(
            error.contains("Formattable") || error.contains("format"),
            "{error}"
        );
    }
}

#[test]
fn native_report_write_is_private() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.plenty");
    std::fs::write(
        &path,
        "def main() -> Result[(), IoError]:\n    drop(__report_write(\"hello\", 0)?)\n    Ok(())\n",
    )
    .unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(error.contains("__report_write"), "{error}");
}

#[cfg(target_os = "linux")]
#[test]
fn nonblocking_stderr_preserves_exact_partial_progress() {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    let directory = tempfile::tempdir().unwrap();
    let source = format!(
        r#"
import std.report
from std.report import FormatError
def main() -> Result[(), FormatError | IoError]:
    text = "{}"
    mut out = std.report.stderr_formatter(2000000, 0)?
    match out.text(&text):
        case Err(_):
            print("failed", out.completed(), out.failed())?
        case Ok(_):
            print("unexpected")?
    match out.text("retry"):
        case Err(error):
            print(error, out.completed())?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#,
        "x".repeat(1_048_576)
    );
    let executable = compile(directory.path(), &source);
    let mut command = Command::new(executable);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    // Linux fcntl constants: F_GETFL=3, F_SETFL=4, O_NONBLOCK=0x800.
    // Only async-signal-safe libc calls execute between fork and exec.
    unsafe {
        command.pre_exec(|| {
            let flags = fcntl(2, 3);
            if flags < 0 || fcntl(2, 4, flags | 0x800) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    assert!(child.wait().unwrap().success());
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    assert!(!stderr.is_empty() && stderr.len() < 1_048_576);
    assert!(stderr.iter().all(|byte| *byte == b'x'));
    assert_eq!(
        stdout,
        format!(
            "failed {} True\nstd.report.FormatError.OutputClosed {}\n",
            stderr.len(),
            stderr.len()
        )
    );
}

#[test]
fn relocated_compiler_embeds_report_module() {
    let directory = tempfile::tempdir().unwrap();
    let compiler = directory.path().join("compiler");
    support::install_executable(Path::new(env!("CARGO_BIN_EXE_plenty")), &compiler);
    let source = directory.path().join("main.plenty");
    std::fs::write(&source, "import std.report\nfrom std.report import FormatError\ndef main() -> Result[(), FormatError]:\n    mut out = std.report.stderr_formatter(20, 0)?\n    out.integer(42)?\n    Ok(())\n").unwrap();
    let executable = directory.path().join("program");
    let result = Command::new(compiler)
        .arg("--compile")
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stderr, b"42");
}

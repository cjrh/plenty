//! Entrypoint reporting selects the trusted protocol and preserves ownership.
use std::process::Command;

const TYPES: &str = r#"
from std.report import Formatter, FormatError
pub class Problem:
    message: str
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        output.text(&self.message)
    def __del__(self) -> ():
        print("dropped").unwrap()
pub enum Code:
    Value(i64)
    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:
        match self:
            case Code.Value(value):
                output.integer(*value)
"#;

fn both(source: &str) -> (tempfile::TempDir, [Command; 2]) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("main.plenty");
    let binary = directory.path().join("program");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &binary, None)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut cli = Command::new(env!("CARGO_BIN_EXE_plenty"));
    cli.arg(file);
    (directory, [Command::new(binary), cli])
}

fn reports(source: &str, stdout: &str, stderr: &str) {
    let (_directory, commands) = both(source);
    for mut command in commands {
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(String::from_utf8_lossy(&output.stdout), stdout);
        assert_eq!(String::from_utf8_lossy(&output.stderr), stderr);
    }
}

#[test]
fn class_enum_and_whole_sum_choose_human_formatting() {
    for (ty, value, stdout, body) in [
        (
            "Problem",
            "Problem('missing input')",
            "dropped\n",
            "missing input",
        ),
        ("Code", "Code.Value(42)", "", "42"),
        ("Problem | Code", "Code.Value(42)", "", "42"),
        (
            "Problem | Code",
            "Problem('missing input')",
            "dropped\n",
            "missing input",
        ),
    ] {
        reports(
            &format!("{TYPES}\ndef main() -> Result[(), {ty}]:\n    Err({value})\n"),
            stdout,
            &format!("error: main returned {body}\n"),
        );
    }
}

#[test]
fn successful_results_do_not_run_reporting() {
    for (ty, value, code) in [("()", "()", 0), ("i32", "23i32", 23)] {
        let source = format!("{TYPES}\ndef main() -> Result[{ty}, Problem]:\n    Ok({value})\n");
        let (_directory, commands) = both(&source);
        for mut command in commands {
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(code));
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn incomplete_sum_and_inexact_or_private_methods_keep_structural_fallback() {
    reports(
        &format!("{TYPES}\ndef main() -> Result[(), Problem | str]:\n    Err(Problem('input'))\n"),
        "dropped\n",
        "error: main returned Problem(message=\"input\")\n",
    );
    for method in [
        "def format(self, output: &mut Formatter) -> Result[(), FormatError]:\n        output.text('human')",
        "pub def format(self, output: &mut Formatter) -> ():\n        pass",
    ] {
        reports(
            &format!("from std.report import Formatter, FormatError\npub class Hidden:\n    value: i64\n    {method}\ndef main() -> Result[(), Hidden]:\n    Err(Hidden(9))\n"),
            "",
            "error: main returned Hidden(value=9)\n",
        );
    }
    reports(
        "pub class Formatter:\n    pass\npub enum FormatError:\n    Failed\npub class Lookalike:\n    value: i64\n    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:\n        Ok(())\ndef main() -> Result[(), Lookalike]:\n    Err(Lookalike(9))\n",
        "",
        "error: main returned Lookalike(value=9)\n",
    );
}

#[test]
fn generic_and_normalized_alias_error_types_select_reporting() {
    reports(
        "from std.report import Formatter, FormatError\npub class Generic[T]:\n    value: T\n    pub def format(self, output: &mut Formatter) -> Result[(), FormatError]:\n        output.text('generic')\ntype Error = Generic[i64] | Generic[i64]\ndef main() -> Result[i32, Error]:\n    Err(Generic[i64](42))\n",
        "",
        "error: main returned generic\n",
    );
}

#[test]
fn nested_typed_context_reports_once_and_unformattable_cause_falls_back() {
    reports(
        &format!("{TYPES}\nfrom std.error import Context, Subject\ndef main() -> Result[(), Context[Context[Problem]]]:\n    inner = Context[Problem]('read', Subject.Absent, Problem('missing'))\n    Err(Context[Context[Problem]]('load', Subject.Absent, inner))\n"),
        "dropped\n",
        "error: main returned load: read: missing\n",
    );
    reports(
        "from std.error import Context, Subject\ndef main() -> Result[(), Context[str]]:\n    Err(Context[str]('read', Subject.Absent, 'missing'))\n",
        "",
        "error: main returned std.error.Context[str](operation=\"read\", subject=std.error.Subject.Absent, cause=\"missing\")\n",
    );
}

#[test]
fn formatting_failure_keeps_partial_output_and_drops_once() {
    for text in ["", "partial"] {
        let source = TYPES.replace(
            "output.text(&self.message)",
            &format!("output.text('{text}')?\n        Err(FormatError.InvalidSpec)"),
        );
        reports(
            &format!("{source}\ndef main() -> Result[i32, Problem]:\n    Err(Problem('unused'))\n"),
            "dropped\n",
            &format!("error: main returned {text} [report unavailable]\n"),
        );
    }
}

#[test]
fn record_budget_includes_prefix_and_newline_and_recursion_is_bounded() {
    let prefix = "error: main returned ";
    for length in [16384 - prefix.len() - 1, 16384 - prefix.len()] {
        let text = "x".repeat(length);
        let ending = if length + prefix.len() == 16384 {
            " [report unavailable]\n"
        } else {
            "\n"
        };
        reports(
            &format!("{TYPES}\ndef main() -> Result[(), Problem]:\n    Err(Problem('{text}'))\n"),
            "dropped\n",
            &format!("{prefix}{text}{ending}"),
        );
    }
    let recursive = TYPES
        .replace(
            "from std.report import Formatter, FormatError",
            "from std.report import Formatter, FormatError, render",
        )
        .replace("output.text(&self.message)", "render(self, output)");
    reports(
        &format!("{recursive}\ndef main() -> Result[(), Problem]:\n    Err(Problem('input'))\n"),
        "dropped\n",
        "error: main returned  [report unavailable]\n",
    );
}

#[cfg(unix)]
#[test]
fn stdout_is_flushed_before_report_and_error_destruction() {
    let source = format!("{TYPES}\ndef main() -> Result[(), Problem]:\n    print('before').unwrap()\n    Err(Problem('input'))\n");
    let (_directory, commands) = both(&source);
    for command in commands {
        let output = Command::new("sh")
            .args(["-c", "exec \"$@\" 2>&1", "sh"])
            .arg(command.get_program())
            .args(command.get_args())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            output.stdout,
            b"before\nerror: main returned input\ndropped\n"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn human_reporting_does_not_need_allocations() {
    reports(
        &format!("{TYPES}\ndef main() -> Result[(), Problem]:\n    error = Problem('input')\n    print('__test_fail_allocations_after_0__').unwrap()\n    Err(error)\n"),
        "__test_fail_allocations_after_0__\ndropped\n",
        "error: main returned input\n",
    );
}

#[cfg(unix)]
#[test]
fn closed_stderr_and_broken_pipe_preserve_cleanup_and_status() {
    let source =
        format!("{TYPES}\ndef main() -> Result[(), Problem]:\n    Err(Problem('input'))\n");
    let (_directory, commands) = both(&source);
    for mut command in commands {
        let output = Command::new("sh")
            .args(["-c", "exec \"$@\" 2>&-", "sh"])
            .arg(command.get_program())
            .args(command.get_args())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"dropped\n");
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let output = command.stderr(writer).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"dropped\n");
    }
}

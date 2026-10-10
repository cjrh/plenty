//! Entrypoint contracts use complete programs, without the fragment test helper.
use std::process::Command;

fn run_both(source: &str, code: i32, stdout: &str) {
    run_both_reporting(source, code, stdout, "");
}

fn run_both_reporting(source: &str, code: i32, stdout: &str, stderr: &str) {
    let (_workspace, commands) = both(source);
    for mut command in commands {
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(code), "{source}");
        assert_eq!(output.stdout, stdout.as_bytes(), "{source}");
        assert_eq!(String::from_utf8_lossy(&output.stderr), stderr, "{source}");
    }
}

/// The compiled executable and `plenty FILE`, each ready to run `source`.
fn both(source: &str) -> (tempfile::TempDir, [Command; 2]) {
    plenty::check_source(source).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("entry.plenty");
    let executable = workspace.path().join("entry");
    std::fs::write(&file, source).unwrap();
    plenty::compile_source_to_executable(source, &executable).unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--check")
        .arg(&file)
        .output()
        .unwrap();
    assert!(checked.status.success());
    assert!(checked.stdout.is_empty());
    assert!(checked.stderr.is_empty());
    let mut through_cli = Command::new(env!("CARGO_BIN_EXE_plenty"));
    through_cli.arg(&file);
    (workspace, [Command::new(&executable), through_cli])
}

/// Run `command` under a shell that first applies `redirection`.
#[cfg(unix)]
fn redirected(command: &Command, redirection: &str) -> std::process::Output {
    Command::new("sh")
        .arg("-c")
        .arg(format!("exec \"$@\" {redirection}"))
        .arg("sh")
        .arg(command.get_program())
        .args(command.get_args())
        .output()
        .unwrap()
}

#[test]
fn main_is_called_once_and_other_declarations_have_no_startup_effects() {
    run_both(
        "def unused() -> ():\n    print('unused').unwrap()\ndef main() -> ():\n    print('started').unwrap()\n",
        0,
        "started\n",
    );
    run_both("def main() -> ():\n    pass\n", 0, "");
}

#[test]
fn main_returns_the_process_status_and_allows_forward_calls_and_aliases() {
    run_both(
        "type Status = i32\ndef main() -> Status:\n    status()\ndef status() -> i32:\n    23i32\n",
        23,
        "",
    );
    run_both(
        "type Done = ()\ndef main() -> Done:\n    if True:\n        return\n    print('unreached').unwrap()\n",
        0,
        "",
    );
}

#[test]
fn main_drops_resources_before_the_native_wrapper_returns() {
    let declarations = "class Resource:\n    name: str\n    def __del__(self) -> ():\n        print(self.name).unwrap()\ndef status() -> i32:\n    print('status').unwrap()\n    7i32\n";
    for tail in ["return status()", "status()"] {
        run_both(
            &format!("{declarations}def main() -> i32:\n    a = Resource('first')\n    b = Resource('second')\n    {tail}\n"),
            7,
            "second\nfirst\nstatus\n",
        );
    }
    run_both(
        &format!("{declarations}def main() -> ():\n    a = Resource('unit')\n    return\n"),
        0,
        "unit\n",
    );
}

#[test]
fn invalid_entrypoints_are_rejected_before_execution_or_artifact_creation() {
    for (source, diagnostic) in [
        ("", "binary application requires"),
        ("# no program\n", "binary application requires"),
        (
            "def helper() -> ():\n    pass\n",
            "binary application requires",
        ),
        (
            "class App:\n    def main(self) -> ():\n        pass\n",
            "binary application requires",
        ),
        ("type main = i32\n", "binary application requires"),
        ("print('must not run').unwrap()\n", "module scope"),
        ("def main() -> ():\n    pass\nmain()\n", "module scope"),
        ("def main() -> ():\n    pass\nx = 1\n", "module scope"),
        (
            "def main(x: i64) -> ():\n    pass\n",
            "main must take no parameters",
        ),
        (
            "def main() -> i64:\n    0\n",
            "main must take no parameters",
        ),
        (
            "def main() -> Result[i64, str]:\n    Ok(0)\n",
            "main must take no parameters",
        ),
        (
            "def main() -> Generator[i64]:\n    yield 1\n",
            "cannot be a generator",
        ),
        ("def main() -> ():\n    yield 1\n", "cannot be a generator"),
        ("def main() -> i32:\n    0i64\n", "expected i32, got i64"),
        ("def main() -> ():\n    42\n", "expected (), got i64"),
        (
            "def main() -> ():\n    pass\ndef main() -> ():\n    pass\n",
            "already defined",
        ),
        (
            "def main() -> ():\n    x = 1\n    helper()\ndef helper() -> ():\n    print(x).unwrap()\n",
            "unknown binding",
        ),
    ] {
        let error = plenty::check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{source}\n{error}");
        let workspace = tempfile::tempdir().unwrap();
        let file = workspace.path().join("bad.plenty");
        let executable = workspace.path().join("bad");
        std::fs::write(&file, source).unwrap();
        let error = plenty::compile_source_to_executable(source, &executable)
            .unwrap_err()
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
        for args in [vec![], vec!["--check"]] {
            let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
                .args(args)
                .arg(&file)
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
        }
        let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg("--compile")
            .arg(&file)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
        assert!(!executable.exists());
    }
}

const RESOURCE: &str = "class Resource:\n    name: str\n    def __del__(self) -> ():\n        print(self.name).unwrap()\n";
const FAILURE_REPORT: &str =
    "error: main returned Failure.Unspecified: Failure keeps no details of the original error\n";
const FIXED_REPORT: &str = "error: main returned Err\n";

#[test]
fn a_returned_err_is_reported_on_stderr_with_status_one() {
    run_both_reporting(
        "def main() -> Result[(), IoError]:\n    text = read_text('missing/input.txt')?\n    print(text)?\n    Ok(())\n",
        1,
        "",
        "error: main returned IoError.System(2): No such file or directory\n",
    );
    run_both_reporting(
        "def main() -> Result[i32, ParseError]:\n    print('before').unwrap()\n    count = i32.parse('many')?\n    Ok(count)\n",
        1,
        "before\n",
        "error: main returned ParseError.Invalid\n",
    );
    run_both_reporting(
        "enum AppError:\n    Missing(str)\ndef main() -> Result[(), AppError]:\n    Err(AppError.Missing('config'))\n",
        1,
        "",
        "error: main returned AppError.Missing(\"config\")\n",
    );
    run_both_reporting(
        "def main() -> Result[(), str]:\n    Err('no input')\n",
        1,
        "",
        "error: main returned \"no input\"\n",
    );
    run_both_reporting(
        "def attempt(mode: str) -> Result[(), IoError]:\n    file = open('entry.plenty', mode)?\n    Ok(())\ndef main() -> Result[(), IoError]:\n    attempt('rw')\n",
        1,
        "",
        "error: main returned IoError.InvalidMode: the open mode must be one of r, w, a, x, r+, w+, a+, x+\n",
    );
}

#[test]
fn successful_result_mains_keep_their_output_and_status() {
    run_both(
        "def main() -> Result[(), IoError]:\n    print('done')?\n    Ok(())\n",
        0,
        "done\n",
    );
    run_both(
        "def main() -> Result[i32, ParseError]:\n    print('done').unwrap()\n    Ok(i32.parse('23')?)\n",
        23,
        "done\n",
    );
}

#[test]
fn failure_is_reported_without_details_it_does_not_keep() {
    for body in [
        "    i32.parse('many')?\n    Ok(())\n",
        "    read_text('missing/input.txt')?\n    Ok(())\n",
        "    Err(Failure.Unspecified)\n",
    ] {
        run_both_reporting(
            &format!("def main() -> Result[(), Failure]:\n{body}"),
            1,
            "",
            FAILURE_REPORT,
        );
    }
}

#[test]
fn an_owned_error_is_reported_and_then_dropped_once() {
    let source = format!("{RESOURCE}def fail() -> Result[(), Resource]:\n    Err(Resource('payload'))\ndef main() -> Result[(), Resource]:\n    local = Resource('local')\n    fail()?\n    Ok(())\n");
    let report = "error: main returned Resource(name=\"payload\")\n";
    run_both_reporting(&source, 1, "local\npayload\n", report);
    #[cfg(unix)]
    for command in both(&source).1 {
        let output = redirected(&command, "2>&1");
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("local\n{report}payload\n")
        );
    }
}

#[test]
fn errors_without_a_rendering_use_the_fixed_message() {
    run_both_reporting(
        "enum Chain:\n    End\n    Link(Box[Chain])\ndef main() -> Result[(), Chain]:\n    Err(Chain.End)\n",
        1,
        "",
        FIXED_REPORT,
    );
    run_both_reporting(
        "def numbers() -> Generator[i64]:\n    yield 1\ndef main() -> Result[(), Generator[i64]]:\n    Err(numbers())\n",
        1,
        "",
        FIXED_REPORT,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn a_report_that_cannot_allocate_uses_the_fixed_message() {
    run_both_reporting(
        &format!("{RESOURCE}def main() -> Result[(), Resource]:\n    error = Resource('payload')\n    print('__test_fail_allocations_after_0__').unwrap()\n    Err(error)\n"),
        1,
        "__test_fail_allocations_after_0__\npayload\n",
        FIXED_REPORT,
    );
}

#[cfg(unix)]
#[test]
fn an_unwritable_stderr_still_runs_cleanup_and_exits_with_one() {
    let source =
        format!("{RESOURCE}def main() -> Result[(), Resource]:\n    Err(Resource('payload'))\n");
    for mut command in both(&source).1 {
        let closed = redirected(&command, "2>&-");
        assert_eq!(closed.status.code(), Some(1));
        assert_eq!(closed.stdout, b"payload\n");

        // Without a reader, the write raises SIGPIPE unless the report ignores it.
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let abandoned = command.stderr(writer).output().unwrap();
        assert_eq!(abandoned.status.code(), Some(1));
        assert_eq!(abandoned.stdout, b"payload\n");
    }
}

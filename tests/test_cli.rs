//! Checked Plenty argument parsing, including native non-UTF-8 argv.
use std::ffi::OsStr;
use std::process::Command;

mod support;

fn run(source: &str, arguments: &[&OsStr], status: i32, expected: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("entry.plenty");
    let executable = workspace.path().join("entry");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &executable, None)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    let output = Command::new(executable).args(arguments).output().unwrap();
    assert_eq!(output.status.code(), Some(status), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let visible: String = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(visible, expected);
}

const IMPORTS: &str = "from std.cli import Spec, Kind, Value, CliError, parse, help, validate, describe, exit_status\n";

#[test]
fn values_are_typed_ordered_and_repeated_without_consuming_input() {
    run(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("--verbose", Kind.Flag, False, False, False, "verbose"), Spec("--count", Kind.Integer, False, True, True, "count"), Spec("FILE", Kind.Path, True, True, True, "input files")]?
    arguments = args_bytes()?
    parsed = parse(&schema, &arguments, 1)?
    print(parsed.help_requested)?
    for entry in &parsed.entries:
        print(entry.spec)?
        match &entry.value:
            case Value.Integer(value):
                print(value)?
            case Value.Path(path):
                print(path.to_text()?)?
            case Value.Flag:
                print("flag")?
            case _:
                print("wrong kind")?
    print(len(arguments))?
    Ok(())
"#
        ),
        &[
            "first".as_ref(),
            "--count".as_ref(),
            "-12".as_ref(),
            "--verbose".as_ref(),
            "--count".as_ref(),
            "4".as_ref(),
            "--".as_ref(),
            "--literal".as_ref(),
        ],
        0,
        "False\n2\nfirst\n1\n-12\n0\nflag\n1\n4\n2\n--literal\n9\n",
    );
}

#[test]
fn errors_and_statuses_are_deterministic() {
    let source = format!(
        r#"{IMPORTS}
def main() -> i32:
    schema = [Spec("--count", Kind.Integer, False, True, False, "count")].unwrap()
    arguments = args_bytes().unwrap()
    match parse(&schema, &arguments, 1):
        case Ok(parsed):
            print(parsed.help_requested).unwrap()
            0i32
        case Err(error):
            print(describe(&error).unwrap()).unwrap()
            exit_status(&error)
"#
    );
    for (args, status, expected) in [
        (vec![], 2, "missing required schema entry 0\n"),
        (vec!["--wat"], 2, "unknown option at argument 1\n"),
        (vec!["file"], 2, "unexpected argument at index 1\n"),
        (vec!["--count"], 2, "missing value for schema entry 0\n"),
        (
            vec!["--count", "x"],
            2,
            "invalid or out-of-range integer at argument 2\n",
        ),
        (
            vec!["--count", "9223372036854775808"],
            2,
            "invalid or out-of-range integer at argument 2\n",
        ),
        (
            vec!["--count", "--help"],
            2,
            "invalid or out-of-range integer at argument 2\n",
        ),
        (
            vec!["--count", "1", "--count", "2"],
            2,
            "duplicate option for schema entry 0\n",
        ),
        (vec!["--help", "--wat"], 0, "True\n"),
        (vec!["--wat", "--help"], 2, "unknown option at argument 1\n"),
        (vec!["--count", "-2"], 0, "False\n"),
    ] {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        run(&source, &args, status, expected);
    }
}

#[test]
fn empty_tokens_negative_positionals_and_termination_are_values() {
    run(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("N", Kind.Integer, True, True, False, "number"), Spec("TEXT", Kind.Text, True, True, True, "text")]?
    arguments = args_bytes()?
    parsed = parse(&schema, &arguments, 1)?
    print(parsed.entries)?
    empty_schema: list[Spec] = []?
    empty_args: list[list[u8]] = []?
    print(len(parse(&empty_schema, &empty_args, 0)?.entries))?
    print(str.repr(parse(&empty_schema, &empty_args, -1))?)?
    print(str.repr(parse(&empty_schema, &empty_args, 1))?)?
    Ok(())
"#
        ),
        &["-7".as_ref(), "".as_ref(), "--".as_ref(), "--help".as_ref()],
        0,
        "[std.cli.Entry(spec=0, value=std.cli.Value.Integer(-7)), std.cli.Entry(spec=1, value=std.cli.Value.Text(\"\")), std.cli.Entry(spec=1, value=std.cli.Value.Text(\"--help\"))]\n0\nResult[std.cli.Parsed, std.cli.CliError].Err(std.cli.CliError.UnexpectedArgument(-1))\nResult[std.cli.Parsed, std.cli.CliError].Err(std.cli.CliError.UnexpectedArgument(1))\n",
    );
}

#[test]
fn schema_errors_precede_help_and_reject_ambiguous_positionals() {
    for schema in [
        "[Spec(\"--help\", Kind.Flag, False, False, False, \"\")]",
        "[Spec(\"-x\", Kind.Flag, False, False, False, \"\")]",
        "[Spec(\"bad name\", Kind.Text, True, False, False, \"\")]",
        "[Spec(\"--x\", Kind.Flag, False, False, False, \"\"), Spec(\"--x\", Kind.Flag, False, False, False, \"\")]",
        "[Spec(\"X\", Kind.Flag, True, False, False, \"\")]",
        "[Spec(\"X\", Kind.Text, True, False, False, \"\"), Spec(\"Y\", Kind.Text, True, True, False, \"\")]",
        "[Spec(\"X\", Kind.Text, True, True, True, \"\"), Spec(\"Y\", Kind.Text, True, False, False, \"\")]",
    ] {
        run(
            &format!(
                r#"{IMPORTS}
def main() -> i32:
    schema = {schema}.unwrap()
    arguments = args_bytes().unwrap()
    match parse(&schema, &arguments, 1):
        case Ok(_):
            3i32
        case Err(error):
            match error:
                case CliError.Schema(_):
                    1i32
                case _:
                    4i32
"#
            ),
            &["--help".as_ref()],
            1,
            "",
        );
    }
}

#[test]
fn help_is_generated_in_schema_order() {
    run(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("--count", Kind.Integer, False, False, False, "maximum rows"), Spec("FILE", Kind.Path, True, True, True, "input files")]?
    drop(write_stdout(help(&schema, "finder")?))
    Ok(())
"#
        ),
        &[],
        0,
        "Usage: finder [--help]\n  --count <integer>  maximum rows\n  FILE <path> (required) (repeatable)  input files\n  --help  Show this help\n",
    );
}

#[test]
fn help_token_is_an_ordinary_text_option_value() {
    run(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("--label", Kind.Text, False, True, False, "label")]?
    raw = args_bytes()?
    parsed = parse(&schema, &raw, 1)?
    print(parsed.help_requested)?
    match &parsed.entries[0].value:
        case Value.Text(text):
            print(text)?
        case _:
            return Err(CliError.UnexpectedArgument(-1))
    Ok(())
"#
        ),
        &["--label".as_ref(), "--help".as_ref()],
        0,
        "False\n--help\n",
    );
}

#[cfg(unix)]
#[test]
fn native_path_arguments_preserve_invalid_utf8_and_text_rejects_it() {
    use std::os::unix::ffi::OsStrExt;
    for (kind, expected) in [
        ("Path", "[97, 255, 122]\n"),
        ("Text", "invalid UTF-8 at argument 1\n"),
    ] {
        run(
            &format!(
                r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("INPUT", Kind.{kind}, True, True, False, "input")]?
    arguments = args_bytes()?
    match parse(&schema, &arguments, 1):
        case Ok(parsed):
            match &parsed.entries[0].value:
                case Value.Path(path):
                    print(path.as_bytes())?
                case _:
                    print("wrong kind")?
        case Err(error):
            print(describe(&error)?)?
    Ok(())
"#
            ),
            &[OsStr::from_bytes(b"a\xffz")],
            0,
            expected,
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn parser_and_help_report_allocation_failure_without_trapping() {
    run(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("--count", Kind.Integer, False, True, False, "count")]?
    arguments = args_bytes()?
    print("__test_fail_allocations_after_0__")?
    parsed = parse(&schema, &arguments, 1)
    rendered = help(&schema, "finder")
    print("__test_restore_allocations__")?
    match parsed:
        case Err(error):
            print(describe(&error)?)?
            print(exit_status(&error))?
        case Ok(_):
            print("unexpected parse success")?
    match rendered:
        case Err(error):
            print(describe(&error)?)?
        case Ok(_):
            print("unexpected help success")?
    Ok(())
"#
        ),
        &["--count".as_ref(), "12".as_ref()],
        0,
        "no memory to parse arguments\n1\nno memory to parse arguments\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn partial_parses_release_owned_values_at_each_allocation_failure() {
    run(
        &format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("--text", Kind.Text, False, True, True, "text"), Spec("FILE", Kind.Path, True, True, True, "file")]?
    raw = [encode_utf8("--text")?, encode_utf8("a long text value that requires heap allocation")?, encode_utf8("a/long/path/that/also/requires/storage")?, encode_utf8("--text")?, encode_utf8("another long text value to allocate")?]?
    mut successes = 0
    mut failures = 0
    for budget in range(40):
        marker = "__test_fail_allocations_after_".concat(&str.repr(budget)?)?.concat("__")?
        print(marker)?
        parsed = parse(&schema, &raw, 0)
        print("__test_restore_allocations__")?
        match parsed:
            case Ok(result):
                if len(result.entries) != 3:
                    return Err(CliError.UnexpectedArgument(-1))
                successes += 1
            case Err(error):
                match error:
                    case CliError.Allocation(_):
                        failures += 1
                    case _:
                        return Err(CliError.UnexpectedArgument(-1))
    print(successes > 0, failures > 0)?
    print(len(raw))?
    Ok(())
"#
        ),
        &[],
        0,
        "True True\n5\n",
    );
}

#[test]
fn parsing_in_workers_uses_the_ordinary_module_checks() {
    run(
        &format!(
            r#"{IMPORTS}
def count(value: i64) -> i64:
    schema = [Spec("N", Kind.Integer, True, True, False, "number")].unwrap()
    raw = [encode_utf8(&str.repr(value).unwrap()).unwrap()].unwrap()
    result = parse(&schema, &raw, 0).unwrap()
    match &result.entries[0].value:
        case Value.Integer(number):
            *number
        case _:
            0

def main() -> Result[(), AllocError | PoolError | SubmitError[OnceClosure[[], i64]] | FutureError | IoError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        first = pool.submit(count, 17)?
        second = pool.submit(count, -42)?
        print(first.result()?, second.result()?)?
    Ok(())
"#
        ),
        &[],
        0,
        "17 -42\n",
    );
}

#[test]
fn relocated_toolchain_parses_without_sources_or_rust_tools() {
    let workspace = tempfile::tempdir().unwrap();
    let compiler = workspace.path().join("compiler");
    let source = workspace.path().join("main.plenty");
    let executable = workspace.path().join("program");
    support::install_executable(
        std::path::Path::new(env!("CARGO_BIN_EXE_plenty")),
        &compiler,
    );
    std::fs::write(
        &source,
        format!(
            r#"{IMPORTS}
def main() -> Result[(), AllocError | CliError | IoError]:
    schema = [Spec("--number", Kind.Integer, False, True, False, "number")]?
    raw = args_bytes()?
    parsed = parse(&schema, &raw, 1)?
    print(len(parsed.entries))?
    Ok(())
"#
        ),
    )
    .unwrap();
    let checked = Command::new(&compiler)
        .arg("--check")
        .arg(&source)
        .env("PATH", "")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(checked.status.success(), "{checked:?}");
    let compiled = Command::new(compiler)
        .arg("--compile")
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(compiled.status.success(), "{compiled:?}");
    let output = Command::new(executable)
        .args(["--number", "42"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"1\n");
}

#[cfg(unix)]
#[test]
fn countbytes_tool_reads_binary_paths_and_reports_usage_statuses() {
    use std::os::unix::ffi::OsStrExt;
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("countbytes");
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/countbytes.plenty");
    plenty::compile_file_to_executable(&source, &executable, None).unwrap();
    let filename = OsStr::from_bytes(b"input\xff");
    std::fs::write(workspace.path().join(filename), b"\0\xff\n").unwrap();
    let counted = Command::new(&executable)
        .arg("--total")
        .arg(filename)
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert_eq!(counted.status.code(), Some(0), "{counted:?}");
    assert_eq!(counted.stdout, b"3 input\\xff\n3 total\n");
    assert!(counted.stderr.is_empty());
    let missing = Command::new(&executable).output().unwrap();
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert_eq!(missing.stderr, b"error: missing required argument FILE\n");
    let help = Command::new(executable).arg("--help").output().unwrap();
    assert_eq!(help.status.code(), Some(0), "{help:?}");
    assert!(help.stdout.starts_with(b"Usage: countbytes [--help]\n"));
    assert!(help.stderr.is_empty());
}

/// Executables the tests install must run even while other test threads spawn
/// children (issue #125). Writing the same scripts in-process failed with
/// ETXTBSY in about 9% of iterations on Linux.
#[cfg(unix)]
#[test]
fn installed_scripts_run_while_other_threads_spawn() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let workspace = tempfile::tempdir().unwrap();
    let stop = AtomicBool::new(false);
    let failures = std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    Command::new("true").status().unwrap();
                }
            });
        }
        // Stop the spawners even if installing a script panics.
        struct Stop<'a>(&'a AtomicBool);
        impl Drop for Stop<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let _stop = Stop(&stop);
        (0..200)
            .filter_map(|index| {
                let script = workspace.path().join(format!("script{index}"));
                support::write_script(&script, "#!/bin/sh\nexit 0\n");
                match Command::new(&script).status() {
                    Ok(status) if status.success() => None,
                    result => Some(format!("{index}: {result:?}")),
                }
            })
            .collect::<Vec<_>>()
    });
    assert!(failures.is_empty(), "{failures:#?}");
}

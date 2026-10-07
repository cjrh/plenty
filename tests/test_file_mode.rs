//! Integration tests for the binary's file-execution mode.
//!
//! `CARGO_BIN_EXE_plenty` is set by Cargo for binary integration tests; it
//! points at the built `plenty` executable. No extra dev-dep needed.

use std::io::Write;
use std::process::Command;

/// Path to the freshly-built `plenty` binary.
fn plenty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_plenty")
}

/// Write `source` to a uniquely-named tempfile and return the path. The
/// caller is responsible for deleting it.
///
/// The nonce combines `process::id()` with an in-process atomic counter
/// rather than `SystemTime::now().as_nanos()` — two parallel test threads
/// can otherwise observe the same nanosecond and collide on the path.
fn write_tempfile(source: &str, label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let path = std::env::temp_dir().join(format!("plenty-test-{label}-{nonce}.plenty"));
    let mut f = std::fs::File::create(&path).expect("create tempfile");
    f.write_all(source.as_bytes()).expect("write tempfile");
    path
}

#[test]
fn a_well_formed_program_runs_to_completion_and_prints_via_dot() {
    let path = write_tempfile("1 2 + .\n", "happy");
    let out = Command::new(plenty_bin())
        .arg("--legacy")
        .arg(&path)
        .output()
        .expect("spawn");
    let _ = std::fs::remove_file(&path);

    assert!(
        out.status.success(),
        "exit was {:?}; stderr was {:?}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[3i64]");
}

#[test]
fn defining_and_calling_a_function_works_from_a_file() {
    let path = write_tempfile(
        r#"
        : double { x i64 -> i64 } "Double an int." x 2 * ;
        21 :double .
        "#,
        "fndef",
    );
    let out = Command::new(plenty_bin())
        .arg("--legacy")
        .arg(&path)
        .output()
        .expect("spawn");
    let _ = std::fs::remove_file(&path);

    assert!(
        out.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[42i64]");
}

#[test]
fn print_consumes_and_renders_one_value_without_a_newline() {
    let path = write_tempfile(r#"1 :print "x" :print true :print"#, "print");
    let out = Command::new(plenty_bin())
        .arg("--legacy")
        .arg(&path)
        .output()
        .expect("spawn");
    let _ = std::fs::remove_file(&path);

    assert!(
        out.status.success(),
        "stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1i64\"x\"true");
}

#[test]
fn a_type_error_exits_nonzero_with_a_diagnostic() {
    // `+` on mixed Int and Str is rejected by the type checker before any
    // op runs, so we expect no stdout output and an `error:` line.
    let path = write_tempfile("1 hello + .\n", "type-error");
    let out = Command::new(plenty_bin())
        .arg("--legacy")
        .arg(&path)
        .output()
        .expect("spawn");
    let _ = std::fs::remove_file(&path);

    assert!(!out.status.success(), "type error should exit non-zero");
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("error:"),
        "stderr was {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_missing_file_exits_nonzero_with_a_diagnostic() {
    let out = Command::new(plenty_bin())
        .arg("--legacy")
        .arg("/nonexistent/plenty/path/that/should/not/exist.plenty")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error:"), "stderr was {stderr:?}");
}

#[test]
fn the_help_flag_prints_usage_and_exits_zero() {
    for flag in ["-h", "--help"] {
        let out = Command::new(plenty_bin())
            .arg("--legacy")
            .arg(flag)
            .output()
            .expect("spawn");
        assert!(out.status.success(), "`{flag}` should exit zero");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("Usage: plenty"),
            "{flag}: stdout was {stdout:?}"
        );
    }
}

#[test]
fn unrecognised_arguments_exit_nonzero() {
    let out = Command::new(plenty_bin())
        .arg("--legacy")
        .args(["foo.plenty", "bar.plenty"])
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "multiple files should be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unrecognised"), "stderr was {stderr:?}");
}

#[test]
fn no_arguments_print_help_without_starting_an_interactive_session() {
    let out = Command::new(plenty_bin()).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage: plenty FILE"));
    assert!(out.stderr.is_empty());
}

#[test]
fn expression_statements_are_discarded_unless_printed() {
    let path = write_tempfile(
        "def main() -> ():\n    40 + 2\n    pass\n",
        "discard-result",
    );
    let out = Command::new(plenty_bin()).arg(&path).output().unwrap();
    let _ = std::fs::remove_file(path);
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn run_command_inherits_stdin() {
    use std::process::Stdio;
    let path = write_tempfile(":readline drop :println", "stdin");
    let mut child = Command::new(plenty_bin())
        .arg("--legacy")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hello native input\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_file(path);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"hello native input\n");
}

#[cfg(unix)]
#[test]
fn run_command_cleans_temporary_files_after_success_and_failure() {
    let workspace = tempfile::tempdir().unwrap();
    let scratch = workspace.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    let source = workspace.path().join("program.plenty");
    for (program, code, stdout, diagnostic) in [
        ("def main() -> ():\n    print(42).unwrap()\n", 0, "42\n", ""),
        (
            "def main() -> ():\n    print(42).unwrap()\n    1 // 0\n    pass\n",
            1,
            "42\n",
            "division by zero",
        ),
        (
            "def main() -> ():\n    print('must not execute').unwrap()\n    missing()\n",
            1,
            "",
            "unknown function",
        ),
    ] {
        std::fs::write(&source, program).unwrap();
        let out = Command::new(plenty_bin())
            .arg("program.plenty")
            .current_dir(workspace.path())
            .env("TMPDIR", &scratch)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(code));
        assert_eq!(String::from_utf8_lossy(&out.stdout), stdout);
        if diagnostic.is_empty() {
            assert!(out.stderr.is_empty());
        } else {
            assert!(String::from_utf8_lossy(&out.stderr).contains(diagnostic));
        }
        assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
    }
}

#[cfg(unix)]
#[test]
fn checking_needs_no_linker_and_missing_linker_failure_cleans_up() {
    let workspace = tempfile::tempdir().unwrap();
    let empty_path = workspace.path().join("empty-path");
    let scratch = workspace.path().join("scratch");
    std::fs::create_dir(&empty_path).unwrap();
    std::fs::create_dir(&scratch).unwrap();
    let source = workspace.path().join("program.plenty");
    std::fs::write(&source, "def main() -> ():\n    print(42).unwrap()\n").unwrap();
    let checked = Command::new(plenty_bin())
        .arg("--check")
        .arg(&source)
        .env("PATH", &empty_path)
        .output()
        .unwrap();
    assert!(checked.status.success());
    assert!(checked.stdout.is_empty());
    assert!(checked.stderr.is_empty());

    let run = Command::new(plenty_bin())
        .arg(&source)
        .env("PATH", &empty_path)
        .env("TMPDIR", &scratch)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(run.stdout.is_empty());
    assert!(String::from_utf8_lossy(&run.stderr).contains("failed to invoke `cc`"));
    assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
}

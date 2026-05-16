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
fn write_tempfile(source: &str, label: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("plenty-test-{label}-{nonce}.plenty"));
    let mut f = std::fs::File::create(&path).expect("create tempfile");
    f.write_all(source.as_bytes()).expect("write tempfile");
    path
}

#[test]
fn a_well_formed_program_runs_to_completion_and_prints_via_dot() {
    let path = write_tempfile("1 2 + .\n", "happy");
    let out = Command::new(plenty_bin()).arg(&path).output().expect("spawn");
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
    let out = Command::new(plenty_bin()).arg(&path).output().expect("spawn");
    let _ = std::fs::remove_file(&path);

    assert!(out.status.success(), "stderr: {:?}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[42i64]");
}

#[test]
fn a_type_error_exits_nonzero_with_a_diagnostic() {
    // `+` on mixed Int and Str is rejected by the type checker before any
    // op runs, so we expect no stdout output and an `error:` line.
    let path = write_tempfile("1 hello + .\n", "type-error");
    let out = Command::new(plenty_bin()).arg(&path).output().expect("spawn");
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
        let out = Command::new(plenty_bin()).arg(flag).output().expect("spawn");
        assert!(out.status.success(), "`{flag}` should exit zero");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Usage: plenty"), "{flag}: stdout was {stdout:?}");
    }
}

#[test]
fn unrecognised_arguments_exit_nonzero() {
    let out = Command::new(plenty_bin())
        .args(["foo.plenty", "bar.plenty"])
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "multiple files should be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unrecognised"), "stderr was {stderr:?}");
}

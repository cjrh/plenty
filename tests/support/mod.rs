#![allow(dead_code)] // Each integration test uses a different subset of helpers.

use std::path::Path;
use std::process::{Command, Output};

/// Copy `source` to a new executable at `destination` (issue #125).
///
/// Linux refuses to exec a file while any process holds it open for writing
/// (ETXTBSY). A child forked by another test thread inherits this process's
/// descriptors until it execs, and O_CLOEXEC cannot close them sooner, so a
/// file written here can stay busy after it is closed. Copying in a `cp`
/// child keeps the only write descriptor in a process nobody else forks.
pub fn install_executable(source: &Path, destination: &Path) {
    let status = Command::new("cp")
        .arg(source)
        .arg(destination)
        .status()
        .unwrap();
    assert!(status.success(), "cp {source:?} {destination:?}: {status}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(destination, mode).unwrap();
    }
}

/// Write an executable script at `path`; see [`install_executable`].
pub fn write_script(path: &Path, contents: &str) {
    let staged = path.with_extension("staged");
    std::fs::write(&staged, contents).unwrap();
    install_executable(&staged, path);
    std::fs::remove_file(staged).unwrap();
}

/// Strings of at most this many UTF-8 bytes are stored inline and never
/// allocate, so allocation-failure tests expect failure only for longer results.
#[allow(dead_code)]
pub const INLINE_MAX: usize = 7;

/// Complete the small statement/declaration fragments used by feature tests.
/// Entrypoint tests and tutorial programs go directly to the public compiler API.
/// This is fixture construction, not a language compatibility mode.
pub fn program(source: &str) -> String {
    if source.lines().any(|line| line.starts_with("def main(")) {
        return source.into();
    }
    let (declarations, statements) = split_fragment(source);
    format!("{declarations}\ndef main() -> ():\n{statements}    pass\n")
}

fn split_fragment(source: &str) -> (String, String) {
    let mut declarations = String::new();
    let mut statements = String::new();
    let mut declaration = false;
    for line in source.lines() {
        if !line.starts_with(char::is_whitespace)
            && !line.starts_with([')', ']', '}'])
            && !line.trim().is_empty()
            && !line.starts_with('#')
        {
            declaration = ["def ", "type ", "class ", "enum ", "protocol "]
                .iter()
                .any(|prefix| line.starts_with(prefix));
        }
        if declaration {
            declarations.push_str(line);
            declarations.push('\n');
        } else {
            statements.push_str("    ");
            statements.push_str(line);
            statements.push('\n');
        }
    }
    (declarations, statements)
}

pub fn check_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    plenty::check_source(&program(source))
}

pub fn compile_source_to_executable(
    source: &str,
    output: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    plenty::compile_source_to_executable(&program(source), output)
}

pub fn run(source: &str) -> Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    compile_source_to_executable(source, &executable)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    Command::new(executable).output().unwrap()
}

/// Put a fragment in a typed function so its final value can be printed by main.
pub fn assert_value(source: &str, ty: &str, expected: &str) {
    let (declarations, statements) = split_fragment(source);
    let call = if ty == "()" {
        "test_result()".to_owned()
    } else {
        "print(test_result()).unwrap()".to_owned()
    };
    let program = format!(
        "{declarations}\ndef test_result() -> {ty}:\n{statements}\ndef main() -> ():\n    {call}\n"
    );
    let output = run(&program);
    assert!(
        output.status.success(),
        "{program}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{program}"
    );
}

// Shared run-and-compare helpers (issue #276). Files import these under their
// historical local names, so test bodies are unchanged.

/// Run a fragment and compare its whole stdout with `expected`.
pub fn assert_stdout(source: &str, expected: &str) {
    let output = run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

/// Run a fragment and compare stdout, minus `__test_` lines, with `expected`
/// after trimming its trailing whitespace.
pub fn assert_visible_stdout(source: &str, expected: &str) {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let visible = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end(), "{source}");
}

/// Like [`assert_visible_stdout`], but `expected` must match exactly.
pub fn assert_filtered_stdout(source: &str, expected: &str) {
    let output = run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        text.lines()
            .filter(|s| !s.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n"),
        expected
    );
}

/// Run a fragment that must succeed and return its stdout.
pub fn run_stdout(source: &str) -> String {
    let output = run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Assert that a process succeeded, reporting its stderr otherwise.
pub fn assert_success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// The diagnostic for a fragment that must be rejected.
pub fn check_diagnostic(source: &str) -> String {
    check_source(source).unwrap_err().to_string()
}

/// The diagnostic for a fragment that must be rejected, with a named panic.
pub fn check_error(source: &str) -> String {
    check_source(source)
        .expect_err("source should be rejected")
        .to_string()
}

/// The diagnostic for a complete program, checked as `main.plenty`, that must
/// be rejected.
pub fn check_file_error(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    match plenty::check_file(&path, None) {
        Ok(()) => panic!("accepted:\n{source}"),
        Err(error) => error.to_string(),
    }
}

/// Write `source` to `dir/main.plenty` and return its path.
pub fn write_main(dir: &Path, source: &str) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    std::fs::write(&path, source).unwrap();
    path
}

/// Write `text` to `path`, creating its parent directories.
pub fn write_file(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Compile a complete program as `dir/main.plenty` into `dir/program`.
pub fn compile_file(dir: &Path, source: &str) -> std::path::PathBuf {
    let path = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{error}\n{source}"));
    executable
}

/// Compile a complete program in a new temporary directory, which the caller
/// keeps alive while it runs the executable.
pub fn build_file(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    (dir, executable)
}

/// Compile and run a complete program; see [`build_file`].
pub fn run_file(source: &str) -> Output {
    let (_dir, executable) = build_file(source);
    std::process::Command::new(executable).output().unwrap()
}

/// Compile and run a complete program that must succeed, returning stdout
/// without `__test_` lines.
pub fn run_file_stdout(source: &str) -> String {
    let directory = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(compile_file(directory.path(), source))
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

/// Compile and run a complete HTTP program with null stdin. Returns stdout
/// with CRLF shown as `|`, `__test_` lines removed and trailing space trimmed.
pub fn http_program_output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\nstatus {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .replace("\r\n", "|")
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

/// Run `prelude` followed by `main`; see [`http_program_output`].
pub fn assert_http_program(prelude: &str, main: &str, expected: &str) {
    assert_eq!(
        http_program_output(&format!("{prelude}{main}")),
        expected.trim_end()
    );
}

/// Lowercase hexadecimal digits for `data`.
pub fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Directory of the test TLS certificates and keys.
pub fn tls_fixtures() -> String {
    format!("{}/tests/fixtures/tls", env!("CARGO_MANIFEST_DIR"))
}

/// Path to the freshly built `plenty` binary.
pub fn plenty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_plenty")
}

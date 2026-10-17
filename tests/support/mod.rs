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

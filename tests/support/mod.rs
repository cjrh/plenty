#![allow(dead_code)] // Each integration test uses a different subset of helpers.

use std::process::{Command, Output};

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

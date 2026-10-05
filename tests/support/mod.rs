use std::process::{Command, Output};

pub fn run(source: &str) -> Output {
    let workspace = tempfile::tempdir().unwrap();
    let executable = workspace.path().join("program");
    plenty::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    Command::new(executable).output().unwrap()
}

/// Put module statements in a typed function so their final value can be
/// observed through print. Keep aliases and function declarations at module scope.
pub fn assert_value(source: &str, ty: &str, expected: &str) {
    let mut declarations = String::new();
    let mut statements = String::new();
    let mut declaration = false;
    for line in source.lines() {
        if !line.starts_with(' ') && !line.starts_with(')') && !line.trim().is_empty() {
            declaration = line.starts_with("def ") || line.starts_with("type ");
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
    let call = if ty == "()" {
        "test_result()".to_owned()
    } else {
        "print(test_result())".to_owned()
    };
    let program = format!("{declarations}\ndef test_result() -> {ty}:\n{statements}\n{call}\n");
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

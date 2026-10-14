use super::*;

/// These fixtures deliberately mark checked functions as embedded code:
/// real std algorithms should not gain contrived callback-then-trap paths
/// merely to exercise diagnostic context restoration.
fn fixture(source: &str) -> String {
    let mut heap = Heap::default();
    let mut program = crate::frontend::compile(source, &mut heap).unwrap();
    for op in &mut program.ops {
        if let Op::DefineFn(name, f) = op {
            let origins = Arc::make_mut(&mut f.origins);
            origins.standard_library = name.starts_with("standard_");
            origins.trap_file = Some(if origins.standard_library {
                "std/fixture.plenty".into()
            } else {
                "user.plenty".into()
            });
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("program");
    compile_ops_to_executable(
        program.ops,
        &heap,
        &executable,
        program.returns_status,
        &crate::CompileOptions::default(),
    )
    .unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn nested_standard_helpers_restore_the_caller_after_a_tail_calling_callback() {
    let error = fixture("def standard_identity(n: i64) -> i64:\n    n\ndef callback(n: i64) -> i64:\n    standard_identity(n)\ndef standard_forward(n: i64) -> i64:\n    callback(n)\ndef standard_inner(n: i64) -> i64:\n    n // 0\ndef standard_outer(n: i64) -> i64:\n    value = standard_forward(n)\n    standard_inner(value)\ndef main() -> Result[(), Failure]:\n    print(standard_outer(1))?\n    Ok(())\n");
    assert_eq!(
        error,
        "error: std/fixture.plenty:8:5: division by zero\nnote: called from user.plenty:13:11\n"
    );
}

#[test]
fn a_standard_generator_reports_the_resume_site() {
    let error = fixture("def standard_values() -> Generator[i64]:\n    yield 1\n    yield 1 // 0\ndef main() -> Result[(), Failure]:\n    for n in standard_values():\n        print(n)?\n    Ok(())\n");
    assert!(
        error.starts_with("error: std/fixture.plenty:3:11: division by zero\n"),
        "{error}"
    );
    assert!(
        error.ends_with("note: called from user.plenty:5:14\n"),
        "{error}"
    );
}

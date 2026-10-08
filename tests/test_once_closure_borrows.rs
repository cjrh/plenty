mod support;
use support::{check_source, run};

#[test]
fn consuming_captures_can_mix_owned_and_local_borrowed_values() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    mut total = 1
    offset = 3
    values = [5, 8]?
    take = def once [values, &mut total, &offset]() -> list[i64]:
        total = total + offset
        values
    moved = take
    result = moved()
    print(total)?
    total = 10
    print(result)?
    print(total)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n[5, 8]\n10\n");
}

#[test]
fn pending_one_shot_calls_keep_capture_loans_through_later_arguments() {
    for operation in ["value", "change(&mut value)"] {
        let source = format!("def change(value: &mut i64) -> i64:\n    *value = 9\n    *value\ndef main() -> ():\n    mut value = 1\n    f = def once [&mut value](arg: i64) -> i64:\n        value + arg\n    f({operation})\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
}

#[test]
fn borrowed_one_shot_closures_cannot_escape_or_pass_by_value() {
    let source = "def escape() -> OnceClosure[[], i64]:\n    value = 1\n    def once [&value]() -> i64:\n        value\ndef main() -> ():\n    pass\n";
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains("cannot escape"), "{error}");
    let source = "def invoke[F: OnceCallable[[], i64]](f: F) -> i64:\n    f()\ndef main() -> ():\n    value = 1\n    f = def once [&value]() -> i64:\n        value\n    invoke(f)\n    pass\n";
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains("by reference"), "{error}");
}

#[test]
fn consuming_a_transitively_borrowing_environment_checks_roots() {
    let source = "def main() -> ():\n    mut value = 1\n    mut inner = def [&mut value]() -> i64:\n        value = value + 1\n        value\n    outer = def once [&mut inner](arg: i64) -> i64:\n        inner() + arg\n    outer(value)\n    pass\n";
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains("borrow"), "{error}");
}

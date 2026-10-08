mod support;
use support::{check_source, run};

#[test]
fn owned_captures_move_into_reusable_inline_environments() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    offset = 4
    values = [10, 20]?
    adjust = def [offset, values](index: i64) -> i64:
        values[index] + offset
    print(adjust(0))?
    moved = adjust
    print(moved(1))?
    print(offset)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "14\n24\n4\n");
}

#[test]
fn owned_captures_enforce_moves_and_explicit_lists() {
    for (body, expected) in [
        ("values = [1]?\n    f = def [values]() -> i64:\n        len(values)\n    print(values)?", "moved"),
        ("value = 1\n    f = def [value, value]() -> i64:\n        value", "duplicate closure capture"),
        ("value = 1\n    f = def [value](value: i64) -> i64:\n        value", "parameter cannot shadow"),
        ("values = [1]?\n    f = def [values]() -> list[i64]:\n        values", "cannot move out of a closure capture"),
    ] {
        let source = format!("def main() -> Result[(), Failure]:\n    {body}\n    Ok(())\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
}

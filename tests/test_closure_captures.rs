mod support;
use support::{check_source, run};

#[test]
fn mutable_owned_captures_retain_state_between_calls() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    count = 0
    values = [10]?
    mut next = def [mut count, mut values](step: i64) -> Result[i64, AllocError]:
        count = count + step
        values.append(count)?
        Ok(len(values) + count)
    print(next(2))?
    print(next(3))?
    print(count)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Result[i64, AllocError].Ok(4)\nResult[i64, AllocError].Ok(8)\n0\n"
    );
}

#[test]
fn closure_mutation_requires_explicit_permission() {
    for (body, expected) in [
        (
            "n = 0\n    f = def [n]() -> ():\n        n = 1\n    f()",
            "exclusive reference",
        ),
        (
            "n = 0\n    f = def [mut n]() -> ():\n        n = 1\n    f()",
            "mut binding",
        ),
        (
            "n = 0\n    mut f = def [mut n]() -> ():\n        mut n = 1\n    f()",
            "redeclare a closure capture",
        ),
    ] {
        let source = format!("def main() -> ():\n    {body}\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

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

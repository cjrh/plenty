mod support;
use support::{check_source, run};

#[test]
fn exclusive_captures_update_the_original_owner() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    mut count = 0
    mut values = [10]?
    mut change = def [&mut count, &mut values](step: i64) -> Result[(), AllocError]:
        count = count + step
        values.append(count)?
        Ok(())
    change(2)?
    change(3)?
    print(count)?
    print(values)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "5\n[10, 2, 5]\n");
}

#[test]
fn exclusive_captures_exclude_competing_access_and_arguments() {
    for access in ["print(count)?", "count = 9", "other = &count"] {
        let source = format!("def main() -> Result[(), Failure]:\n    mut count = 0\n    mut change = def [&mut count]() -> ():\n        count = count + 1\n    {access}\n    change()\n    Ok(())\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
    let error = check_source("def main() -> ():\n    count = 0\n    mut change = def [&mut count]() -> ():\n        count = 1\n    change()\n").unwrap_err().to_string();
    assert!(error.contains("mut binding"), "{error}");
    let error = check_source("def main() -> ():\n    mut count = 0\n    mut change = def [&mut count](other: &i64) -> ():\n        count = 1\n    change(&count)\n").unwrap_err().to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn shared_captures_borrow_owners_until_the_last_closure_use() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    mut values = [4]?
    read = def [&values]() -> i64:
        values[0]
    print(read())?
    moved = read
    print(moved())?
    values.append(5)?
    print(values)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n4\n[4, 5]\n");
}

#[test]
fn shared_capture_loans_survive_moves_and_control_flow() {
    for action in ["values.append(2)?", "drop(values)"] {
        let source = format!("def main() -> Result[(), Failure]:\n    mut values = [1]?\n    read = def [&values]() -> i64:\n        len(values)\n    moved = read\n    {action}\n    print(moved())?\n    Ok(())\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
    let error = check_source("def main() -> Result[(), Failure]:\n    values = [1]?\n    read = def [&values]() -> i64:\n        len(values)\n    wrapped = Some(read)\n    Ok(())\n").unwrap_err().to_string();
    assert!(error.contains("references cannot be stored"), "{error}");
}

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

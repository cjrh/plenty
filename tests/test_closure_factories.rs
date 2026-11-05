mod support;
use support::{check_source, run};

#[test]
fn borrowed_environments_pass_safely_through_nested_consumers() {
    let output = run(r#"
def twice(f: &mut Closure[[], Result[i64, OverflowError[i64]]]) -> Result[i64, OverflowError[i64]]:
    Ok(f()? + f()?)
def main() -> Result[(), IoError | OverflowError[i64]]:
    mut value = 3
    mut inner = def [&mut value]() -> Result[i64, OverflowError[i64]]:
        value = value + 1
        Ok(value)
    mut outer = def [&mut inner]() -> Result[i64, OverflowError[i64]]:
        inner()
    print(twice(&mut outer)?)?
    print(value)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9\n5\n");
}

#[test]
fn environment_borrows_keep_transitive_capture_loans_live() {
    let error = check_source("def consume[T](f: Closure[[], i64], marker: T) -> i64:\n    f()\ndef main() -> ():\n    n = 0\n    f = def [&n]() -> i64:\n        n\n    consume(f, 1)\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("by reference"), "{error}");
    for action in ["print(value)?", "value = 9"] {
        let source = format!(
            "def apply(f: &mut Closure[[], Result[i64, OverflowError[i64]]]) -> Result[i64, OverflowError[i64]]:\n    f()\ndef main() -> Result[(), IoError | OverflowError[i64]]:\n    mut value = 0\n    mut inner = def [&mut value]() -> Result[i64, OverflowError[i64]]:\n        value = value + 1\n        Ok(value)\n    mut outer = def [&mut inner]() -> Result[i64, OverflowError[i64]]:\n        inner()\n    {action}\n    print(apply(&mut outer)?)?\n    Ok(())\n"
        );
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
    let error = check_source("def apply(f: &mut Closure[[], Result[i64, OverflowError[i64]]], n: &i64) -> Result[i64, OverflowError[i64]]:\n    Ok(f()? + *n)\ndef main() -> ():\n    mut n = 0\n    mut f = def [&mut n]() -> Result[i64, OverflowError[i64]]:\n        n = n + 1\n        Ok(n)\n    drop(apply(&mut f, &n))\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn owned_closures_escape_into_caller_storage() {
    let output = run(r#"
def counter(start: i64) -> Closure[[], Result[i64, OverflowError[i64]]]:
    def [mut start]() -> Result[i64, OverflowError[i64]]:
        start = start + 1
        Ok(start)
def apply(f: &mut Closure[[], Result[i64, OverflowError[i64]]]) -> Result[i64, OverflowError[i64]]:
    f()
def main() -> Result[(), IoError | OverflowError[i64]]:
    mut a = counter(10)
    mut b: Closure[[], Result[i64, OverflowError[i64]]] = counter(30)
    print(apply(&mut a)?)?
    print(b()?)?
    print(a()?)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "11\n31\n12\n");
}

#[test]
fn closure_factories_reject_dangling_captures_and_layout_erasure() {
    let error = check_source("def bad() -> Closure[[], i64]:\n    value = 1\n    def [&value]() -> i64:\n        value\ndef main() -> ():\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("cannot escape"), "{error}");
    let error = check_source("def bad(flag: bool) -> Closure[[], Result[i64, OverflowError[i64]]]:\n    n = 1\n    if flag:\n        return def [n]() -> Result[i64, OverflowError[i64]]:\n            Ok(n)\n    return def [n]() -> Result[i64, OverflowError[i64]]:\n        Ok(n + 1)\ndef main() -> ():\n    pass\n").unwrap_err().to_string();
    assert!(
        error.contains("expected") || error.contains("mismatch"),
        "{error}"
    );
}

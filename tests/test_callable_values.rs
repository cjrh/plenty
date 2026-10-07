mod support;
use support::{check_source, run};

#[test]
fn named_functions_are_structural_copyable_values() {
    let output = run(r#"
def double(value: i64) -> i64:
    value * 2
def increment(other: i64) -> i64:
    other + 1
def apply(operation: Callable[[i64], i64], value: i64) -> i64:
    operation(value)
def choose(first: bool) -> Callable[[i64], i64]:
    double if first else increment
def main() -> Result[(), Failure]:
    mut operation: Callable[[i64], i64] = double
    saved = operation
    operation = increment
    print(apply(operation, 20))?
    print(saved(20))?
    selected = choose(True)
    print(selected(7))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "21\n40\n14\n");
}

#[test]
fn callable_signatures_check_arguments_and_results() {
    for (source, diagnostic) in [
        (
            "def f(x: i64) -> i64:\n    x\ncallback: Callable[[i32], i64] = f",
            "Callable",
        ),
        (
            "def f(x: i64) -> i64:\n    x\ncallback = f\ncallback()",
            "expects 1 arguments",
        ),
        (
            "def f(x: i64) -> i64:\n    x\ncallback = f\ncallback(True)",
            "expected i64",
        ),
        ("callback = 1\ncallback(2)", "not callable"),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

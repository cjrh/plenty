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

#[test]
fn call_results_groups_indices_and_fields() {
    let output = run(r#"
def double(value: i64) -> i64:
    value * 2
def choose() -> Callable[[i64], i64]:
    double
class Handler:
    operation: Callable[[i64], i64]
def main() -> Result[(), Failure]:
    print(choose()(3))?
    print((double)(4))?
    handlers = [double]?
    index = 0
    print(handlers[index](5))?
    print(handlers[0](6))?
    handler = Handler(double)?
    print(handler.operation(7))?
    print(Some(double).unwrap()(8))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "6\n8\n10\n12\n14\n16\n"
    );
}

#[test]
fn indirect_calls_preserve_borrows_and_owned_arguments() {
    let output = run(r#"
def increment(value: &mut i64) -> ():
    *value = *value + 1
def total(values: &list[i64]) -> i64:
    mut sum = 0
    for value in values:
        sum = sum + value
    sum
def consume(values: list[i64]) -> i64:
    len(values)
def main() -> Result[(), Failure]:
    mutate: Callable[[&mut i64], ()] = increment
    mut value = 3
    mutate(&mut value)
    print(value)?
    values = [2, 5]?
    inspect: Callable[[&list[i64]], i64] = total
    print(inspect(&values))?
    take = consume
    print(take(values))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n7\n2\n");
    for source in [
        "def use(a: &mut i64, b: &i64) -> ():\n    pass\ncallback = use\nmut x = 1\ncallback(&mut x, &x)",
        "def use(a: list[i64]) -> ():\n    pass\ncallback = use\nx = [1].unwrap()\ncallback(x)\nprint(x).unwrap()",
    ] {
        assert!(check_source(source).is_err(), "{source}");
    }
}

mod support;
use support::{check_source, run};

#[test]
fn multiline_functions_have_independent_control_flow_and_locals() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    transform = def(value: i64) -> i64:
        mut result = 0
        for n in range(value):
            if n == 2:
                continue
            result = result + n
        if result > 10:
            return 10
        result
    print(transform(5))?
    print(transform(8))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "8\n10\n");
}

#[test]
fn anonymous_functions_escape_without_allocating_an_environment() {
    let output = run(r#"
def make[T]() -> Callable[[T], T]:
    return def(value: T) -> T:
        value
def nested() -> Callable[[], Callable[[i64], i64]]:
    def() -> Callable[[i64], i64]:
        def(value: i64) -> i64:
            value + 1
def main() -> Result[(), Failure]:
    print(make[u8]()(42))?
    print(make[str]()("hello"))?
    print(nested()()(5))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\nhello\n6\n");
}

#[test]
fn anonymous_functions_reject_implicit_capture_and_missing_types() {
    for (source, diagnostic) in [
        ("def main() -> ():\n    value = 1\n    f = def() -> i64:\n        value\n    pass", "cannot capture `value`"),
        ("def top(x: i64) -> i64:\n    x\ndef main() -> ():\n    top = 1\n    f = def(x: i64) -> i64:\n        top(x)\n    pass", "cannot capture `top`"),
        ("def main() -> ():\n    f = def(value) -> i64:\n        value\n    pass", "expected `:`"),
        ("def main() -> ():\n    f = def(value: i64):\n        value\n    pass", "expected `->`"),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

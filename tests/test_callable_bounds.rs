mod support;
use support::{check_source, run};

#[test]
fn callable_bounds_accept_functions_and_owned_environments() {
    let output = run(r#"
def twice[F: Callable[[i64], i64]](f: &F, value: i64) -> i64:
    f(f(value))
def increment(value: i64) -> i64:
    value + 1
def main() -> Result[(), Failure]:
    function = increment
    offset = 3
    closure = def [offset](value: i64) -> i64:
        value + offset
    print(twice(&function, 10))?
    print(twice(&closure, 10))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "12\n16\n");
}

#[test]
fn callable_bounds_validate_signatures_before_lowering_the_body() {
    for expression in ["1", "wrong"] {
        let source = format!("def unused[F: Callable[[i64], i64]](f: F) -> ():\n    pass\ndef wrong(x: u8) -> u8:\n    x\ndef main() -> ():\n    unused({expression})\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("does not satisfy Callable"), "{error}");
    }
}

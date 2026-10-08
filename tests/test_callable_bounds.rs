mod support;
use support::{check_source, run};

#[test]
fn callback_signatures_infer_input_and_output_parameters() {
    let output = run(r#"
def apply[T, U, F: Callable[[T], U]](f: &F, value: T) -> U:
    f(value)
def read[T, F: Callable[[], T]](f: &F) -> T:
    f()
def widen(value: u8) -> i64:
    i64(value)
def main() -> Result[(), Failure]:
    function = widen
    print(apply(&function, 7u8))?
    value = 12u16
    closure = def [value]() -> u16:
        value
    result: u16 = read(&closure)
    print(result)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n12\n");
}

#[test]
fn callable_signature_evidence_must_agree_with_other_arguments() {
    let source = "def apply[T, F: Callable[[T], T]](f: &F, value: T) -> T:\n    f(value)\ndef byte(value: u8) -> u8:\n    value\ndef main() -> ():\n    f = byte\n    apply(&f, 1i64)\n    pass\n";
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains("conflicting types for `T`"), "{error}");
}

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

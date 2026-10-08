mod support;
use support::{check_source, run};

#[test]
fn infer_signature_parameters_from_closure_inputs_and_results() {
    let output = run(r#"
def apply[T](f: &Closure[[T], T], value: T) -> T:
    f(value)
def read[T](f: &Closure[[], T]) -> T:
    f()
def make[T: IntType](offset: T) -> Closure[[T], T]:
    def [offset](value: T) -> T:
        offset + value
def main() -> Result[(), Failure]:
    add = make(2u8)
    print(apply(&add, 3u8))?
    base = 7u16
    get = def [&base]() -> u16:
        base + 1
    print(read(&get))?
    print(1 + get())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "5\n8\n9\n");
}

#[test]
fn owned_environments_can_be_concrete_generic_values() {
    let output = run(r#"
def identity[T](value: T) -> T:
    value
def main() -> Result[(), Failure]:
    value = 6
    original = def [value]() -> i64:
        value
    moved = identity(original)
    print(moved())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "6\n");
}

#[test]
fn signature_evidence_must_agree_without_erasing_capture_lifetimes() {
    let error = check_source("def apply[T](f: &Closure[[T], T], x: T) -> T:\n    f(x)\ndef main() -> ():\n    n = 1u8\n    f = def [n](x: u8) -> u8:\n        n + x\n    apply(&f, 2i64)\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("conflicting types"), "{error}");
    let error = check_source("def identity[T](value: T) -> T:\n    value\ndef main() -> ():\n    n = 1\n    f = def [&n]() -> i64:\n        n\n    g = identity(f)\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("reference type arguments"), "{error}");
}

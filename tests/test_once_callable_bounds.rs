mod support;
use support::{check_source, run};

#[test]
fn consuming_api_accepts_every_callable_ownership_mode() {
    let output = run(r#"
def invoke[T, U, F: OnceCallable[[T], U]](f: F, value: T) -> U:
    mut callback = f
    callback(value)
def double(value: i64) -> i64:
    value * 2
def main() -> Result[(), Failure]:
    print(invoke(double, 5))?
    offset = 3
    add = def [offset](value: i64) -> i64:
        offset + value
    print(invoke(add, 5))?
    total = 4
    change = def [mut total](value: i64) -> i64:
        total = total + value
        total
    print(invoke(change, 5))?
    values = [7]?
    take = def once [values](value: i64) -> list[i64]:
        values
    print(invoke(take, 5))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "10\n8\n9\n[7]\n");
}

#[test]
fn one_shot_constraints_do_not_silence_move_checking() {
    let source = "def twice[F: OnceCallable[[], i64]](f: F) -> i64:\n    f() + f()\ndef main() -> ():\n    f = def once () -> i64:\n        1\n    twice(f)\n    pass\n";
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains("moved"), "{error}");
    let source = "def ignore[F: Callable[[], i64]](f: F) -> ():\n    pass\ndef main() -> ():\n    f = def once () -> i64:\n        1\n    ignore(f)\n";
    let error = check_source(source).unwrap_err().to_string();
    assert!(error.contains("does not satisfy Callable"), "{error}");
}

#[test]
fn consuming_constraint_contextualizes_generic_function_values() {
    let output = run("def identity[T](value: T) -> T:\n    value\ndef run[F: OnceCallable[[u8], u8]](f: F) -> u8:\n    f(9)\ndef main() -> ():\n    print(run(identity)).unwrap()\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9\n");
}

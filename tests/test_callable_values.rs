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

#[test]
fn returned_callable_references_keep_their_origin_alive() {
    let output = run(r#"
def identity(value: &mut i64) -> &mut i64:
    value
def factory() -> Callable[[&mut i64], &mut i64]:
    identity
def main() -> Result[(), Failure]:
    mut value = 4
    alias = factory()(&mut value)
    *alias = 12
    print(value)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "12\n");
    for (source, diagnostic) in [
        ("def identity(value: &mut i64) -> &mut i64:\n    value\ncallback = identity\nmut x = 1\nalias = callback(&mut x)\nx = 2\nprint(alias).unwrap()", "conflicting borrow"),
        ("def escape() -> &i64:\n    x = 1\n    x", "exactly one reference parameter"),
        ("type Invalid = Callable[[&i64, &i64], &i64]", "exactly one reference parameter"),
        ("type Invalid = Callable[[&i64], &mut i64]", "mutable reference parameter"),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

#[test]
fn returned_callable_projections_conservatively_borrow_the_whole_origin() {
    let error = check_source(
        r#"
class Pair:
    left: i64
    right: i64
def left(value: &mut Pair) -> &mut i64:
    &mut value.left
mut pair = Pair(1, 2).unwrap()
select = left
reference = select(&mut pair)
pair.right = 4
print(reference).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("conflicting borrow"), "{error}");
}

#[test]
fn generic_parameters_are_inferred_from_callable_signatures() {
    let output = run(r#"
def increment(value: u8) -> u8:
    value + 1
def apply[T](operation: Callable[[T], T], value: T) -> T:
    operation(value)
def keep[T](value: T) -> T:
    value
def display(value: i64) -> ():
    print(value).unwrap()
def invoke[T](operation: Callable[[T], ()], value: T) -> ():
    operation(value)
def main() -> Result[(), Failure]:
    saved = keep(increment)
    print(apply(saved, 41u8))?
    invoke(display, 7)
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n7\n");
    let error = check_source("def small(value: u8) -> u8:\n    value\ndef apply[T](op: Callable[[T], T], x: T) -> T:\n    op(x)\napply(small, 1i64)").unwrap_err().to_string();
    assert!(error.contains("conflicting types for `T`"), "{error}");
}

#[test]
fn specialized_generic_functions_are_values() {
    let output = run(r#"
def identity[T](value: T) -> T:
    value
def first[A, B](value: A, ignored: B) -> A:
    value
def make[T]() -> Callable[[T], T]:
    identity[T]
def main() -> Result[(), Failure]:
    saved = identity[u8]
    print(saved(42))?
    print((first[i32, bool])(7, True))?
    print(make[str]()("hello"))?
    handlers = [saved]?
    index = 0
    selected = handlers[index]
    print(selected(9))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n7\nhello\n9\n");
    for (source, diagnostic) in [
        (
            "def identity[T](x: T) -> T:\n    x\nsaved = identity",
            "explicit type arguments",
        ),
        (
            "def identity[T: IntType](x: T) -> T:\n    x\nsaved = identity[str]",
            "does not satisfy IntType",
        ),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

#[test]
fn expected_callable_types_select_generic_function_specializations() {
    let output = run(r#"
def identity[T](value: T) -> T:
    value
def select() -> Callable[[u8], u8]:
    identity
def apply(operation: Callable[[u8], u8]) -> u8:
    operation(42)
def main() -> Result[(), Failure]:
    keep: Callable[[u8], u8] = identity
    print(keep(7))?
    print(apply(identity))?
    print(select()(9))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n42\n9\n");
    for (source, diagnostic) in [
        (
            "def identity[T](x: T) -> T:\n    x\nf: Callable[[u8], i64] = identity",
            "conflicting types for `T`",
        ),
        (
            "def identity[T: IntType](x: T) -> T:\n    x\nf: Callable[[str], str] = identity",
            "does not satisfy IntType",
        ),
        (
            "def unused[T]() -> i64:\n    1\nf: Callable[[], i64] = unused",
            "cannot infer type parameter `T`",
        ),
    ] {
        let error = check_source(source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

mod support;
use support::{check_source, run};

#[test]
fn one_shot_factories_preserve_inline_identity_through_sums() {
    let output = run(r#"
def make[T](value: T) -> OnceClosure[[], T]:
    def once [value]() -> T:
        value
def wrap(values: list[i64]) -> Result[OnceClosure[[], list[i64]], AllocError]:
    Ok(make(values))
def call[T](callback: OnceClosure[[], T]) -> T:
    callback()
def main() -> Result[(), Failure]:
    first: OnceClosure[[], u8] = make(12u8)
    print(call(first))?
    wrapped = wrap([3, 4]?)?
    print(wrapped())?
    optional = Some(make(5))
    print(optional.unwrap()())?
    print(make(8)())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "12\n[3, 4]\n5\n8\n"
    );
}

#[test]
fn one_shot_and_reusable_annotations_do_not_unify() {
    for (annotation, modifier) in [("Closure", "once "), ("OnceClosure", "")] {
        let source = format!("def main() -> ():\n    value = 1\n    f: {annotation}[[], i64] = def {modifier}[value]() -> i64:\n        value\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("expected") && error.contains("OnceClosure"),
            "{error}"
        );
    }
}

#[test]
fn one_shot_calls_transfer_captures_to_the_body() {
    let output = run(r#"
def main() -> Result[(), Failure]:
    values = [2, 3]?
    take = def once [values]() -> list[i64]:
        values
    result = take()
    print(result)?
    value = 7
    adjust = def once [mut value](offset: i64) -> i64:
        value = value + offset
        value
    print(adjust(5))?
    empty = def once () -> i64:
        9
    print(empty())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "[2, 3]\n12\n9\n");
}

#[test]
fn one_shot_values_cannot_be_called_twice_or_through_references() {
    for statement in ["take()\n    take()", "alias = &take\n    alias()"] {
        let source = format!("def main() -> ():\n    value = 1\n    take = def once [value]() -> i64:\n        value\n    {statement}\n    pass\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("moved") || error.contains("must be owned"),
            "{error}"
        );
    }
}

#[test]
fn called_and_unused_one_shot_environments_drop_exactly_once() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def main() -> Result[(), Failure]:
    first = Resource(1)
    second = Resource(2)
    take = def once [first, second]() -> Resource:
        first
    result = take()
    print(result.id)?
    drop(result)
    third = Resource(3)
    unused = def once [third]() -> Resource:
        third
    drop(unused)
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "2\n1\n1\n3\n");
}

mod support;
use support::{check_source, run};

#[test]
fn nested_owned_environments_relocate_all_inline_fields() {
    let output = run(r#"
def make(base: i64) -> Closure[[], i64]:
    span = range(base, base + 3)
    inner = def [span]() -> i64:
        span[1]
    def [inner, mut base]() -> i64:
        base = base + inner()
        base
def maybe(base: i64) -> Result[Option[Closure[[], i64]], AllocError]:
    if base < 0:
        return Ok(Nothing)
    Ok(Some(make(base)))
def main() -> Result[(), Failure]:
    mut f = maybe(3)?.unwrap()
    print(f())?
    print(f())?
    print(f)?
    match maybe(-1)?:
        case Nothing:
            print(True)?
        case Some(_):
            print(False)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "7\n11\n<closure>\nTrue\n"
    );
}

#[test]
fn owned_capture_destruction_is_exactly_once_in_reverse_order() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def main() -> Result[(), Failure]:
    a = Resource(1)?
    b = Resource(2)?
    inner = def [a, b]() -> i64:
        a.id + b.id
    outer = def [inner]() -> i64:
        inner()
    print(outer())?
    moved = Some(outer)
    drop(moved)
    print(9)?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n2\n1\n9\n");
}

#[test]
fn owned_nesting_does_not_hide_a_borrowed_capture() {
    let error = check_source("def main() -> ():\n    n = 1\n    f = def [&n]() -> i64:\n        n\n    outer = def [f]() -> i64:\n        f()\n    pass\n").unwrap_err().to_string();
    assert!(error.contains("cannot be captured"), "{error}");
}

//! Recoverable class storage allocation and constructor ownership.
mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let visible = text
        .lines()
        .filter(|s| !s.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim());
}

#[test]
fn generated_and_custom_constructors_are_fallible() {
    native(
        r#"
class Point:
    x: i64
class Number:
    value: i64
    def __init__(self: &mut Number, n: i64) -> ():
        self.value = n * 2
type Alias = Point
def build() -> Result[Point, AllocError]:
    Ok(Alias.new(7)?)
print(build()).unwrap()
print(Number.new(4)).unwrap()
"#,
        "Result[Point, AllocError].Ok(Point(x=7))\nResult[Number, AllocError].Ok(Number(value=8))",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn partial_initialization_releases_fields_without_running_the_class_hook() {
    for budget in 0..=3 {
        native(
            &format!(
                r#"
class Buffer:
    first: list[i64]
    second: list[i64]
    def __init__(self: &mut Buffer) -> Result[(), AllocError]:
        self.first = list[i64].new()?
        self.second = list[i64].new()?
        Ok(())
    def __del__(self: &mut Buffer) -> ():
        print("complete drop").unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = Buffer.new()
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(value):
        print("ready").unwrap()
    case Err(error):
        print(error).unwrap()
drop(Buffer.new())
"#
            ),
            if budget == 3 {
                "ready\ncomplete drop\ncomplete drop"
            } else {
                "AllocError.OutOfMemory\ncomplete drop"
            },
        );
    }
}

#[test]
fn initializer_can_reject_before_initializing_fields() {
    native(r#"
class Number:
    value: i64
    def __init__(self: &mut Number, n: i64) -> Result[(), AllocError]:
        if n < 0:
            return Err(AllocError.CapacityOverflow)
        self.value = n
        Ok(())
print(Number.new(-1)).unwrap()
print(Number.new(4)).unwrap()
"#, "Result[Number, AllocError].Err(AllocError.CapacityOverflow)\nResult[Number, AllocError].Ok(Number(value=4))");
}

#[test]
fn fallible_initialization_still_requires_complete_success_and_checked_calls() {
    for (body, use_site, message) in [
        ("return Ok(())", "drop(C.new())", "fields not initialized"),
        (
            "self.n = self.n\n        Ok(())",
            "drop(C.new())",
            "not initialized",
        ),
    ] {
        let source = format!("class C:\n    n: i64\n    def __init__(self) -> Result[(), AllocError]:\n        {body}\n{use_site}");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn unused_fallible_constructor_does_not_reduce_the_class_depth_limit() {
    let mut source = String::from("class C0:\n    n: i64\n");
    for i in 1..64 {
        source.push_str(&format!("class C{i}:\n    value: C{}\n", i - 1));
    }
    support::check_source(&source).unwrap();
    source.push_str("C63.new(0)\n");
    let error = support::check_source(&source).unwrap_err().to_string();
    assert!(error.contains("type nesting exceeds"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_storage_drops_arguments_without_initializing_an_instance() {
    native(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print("__test_restore_allocations__").unwrap()
        print(self.n).unwrap()
class Owner:
    resource: Resource
    def __init__(self: &mut Owner, r: Resource) -> ():
        print("init").unwrap()
        self.resource = r
    def __del__(self: &mut Owner) -> ():
        print("owner drop").unwrap()
resource = Resource(42).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
result = Owner.new(resource)
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(value):
        print("unexpected").unwrap()
    case Err(error):
        print(error).unwrap()
drop(Owner.new(Resource(7).unwrap()))
"#,
        "42\nAllocError.OutOfMemory\ninit\nowner drop\n7",
    );
}

//! Inline class storage needs no allocation; only a fallible `__init__` can fail.
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
fn generated_and_custom_constructors_return_the_instance() {
    native(
        r#"
class Point:
    x: i64
class Number:
    value: i64
    def __init__(self: &mut Number, n: i64) -> Result[(), OverflowError[i64]]:
        self.value = n * 2
        Ok(())
type Alias = Point
def build() -> Point:
    Alias.new(7)
print(build()).unwrap()
print(Number.new(4).unwrap()).unwrap()
"#,
        "Point(x=7)\nNumber(value=8)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn partial_initialization_releases_fields_without_running_the_class_hook() {
    // Each list allocates once; the instance itself needs no allocation.
    for budget in 0..=2 {
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
            if budget == 2 {
                "ready\ncomplete drop\ncomplete drop"
            } else {
                "AllocError.OutOfMemory\ncomplete drop"
            },
        );
    }
}

#[test]
fn initializer_can_reject_before_initializing_fields() {
    native(
        r#"
class Number:
    value: i64
    def __init__(self: &mut Number, n: i64) -> Result[(), AllocError]:
        if n < 0:
            return Err(AllocError.CapacityOverflow)
        self.value = n
        Ok(())
print(str.repr(Number.new(-1)).unwrap()).unwrap()
print(str.repr(Number.new(4)).unwrap()).unwrap()
"#,
        "Result[Number, AllocError].Err(AllocError.CapacityOverflow)\nResult[Number, AllocError].Ok(Number(value=4))",
    );
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
        let source = format!(
            "class C:\n    n: i64\n    def __init__(self) -> Result[(), AllocError]:\n        {body}\n{use_site}"
        );
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
fn construction_moves_arguments_without_allocating() {
    native(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print(self.n).unwrap()
class Owner:
    resource: Resource
    def __init__(self: &mut Owner, r: Resource) -> ():
        print("init").unwrap()
        self.resource = r
    def __del__(self: &mut Owner) -> ():
        print("owner drop").unwrap()
resource = Resource(42)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
owner = Owner.new(resource)
pair = (Resource(7), 8)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
drop(owner)
"#,
        "init\nowner drop\n42\n7",
    );
}

#[test]
fn constructor_error_contract_is_precise_and_independent_of_caller_mode() {
    native(
        r#"
class Offset:
    value: i64
    def __init__(self, n: i64, step: i64) -> Result[(), OverflowError[i64]]:
        self.value = n + step
        Ok(())
def main() -> Result[(), IoError]:
    direct: Result[Offset, OverflowError[i64]] = Offset(i64.MAX, 1)
    checked_call = checked:
        ignored = len("") * 2
        Offset.new(i64.MIN, -1)
    unchecked_call = unchecked:
        Offset(i64.MAX, 1)
    for result in [direct, checked_call.unwrap(), unchecked_call].unwrap():
        match result:
            case Err(error):
                print(error.wrapped)?
            case Ok(_):
                print("unexpected")?
    print(Offset(2, 3).unwrap().value)?
    Ok(())
"#,
        "-9223372036854775808\n9223372036854775807\n-9223372036854775808\n5",
    );
}

#[test]
fn initializer_return_shapes_and_missing_errors_have_targeted_diagnostics() {
    for output in ["i64", "Result[i64, str]", "Option[()]"] {
        let error = support::check_source(&format!(
            "class C:\n    def __init__(self) -> {output}:\n        pass\n"
        ))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("__init__ must return () or Result[(), E]"),
            "{error}"
        );
    }
    for (body, output, expected) in [
        (
            "self.n = n + 1\n        Ok(())",
            "Result[(), str]",
            "OverflowError[i64]",
        ),
        ("self.n = n", "Result[(), OverflowError[i64]]", "Result"),
        (
            "self.n = n\n        Ok(())",
            "Result[(), OverflowError[i64]]",
            "OverflowError[i64]",
        ),
    ] {
        let call = if body.ends_with("Ok(())") && output.contains("OverflowError") {
            "def wrong() -> Result[C, AllocError]:\n    C(0)?\n"
        } else {
            ""
        };
        let error = support::check_source(&format!(
            "class C:\n    n: i64\n    def __init__(self, n: i64) -> {output}:\n        {body}\n{call}"
        )).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
    for method in ["__del__", "__exit__"] {
        let usage = if method == "__exit__" {
            "    def __enter__(self: &mut C) -> ():\n        pass\nwith C():\n    pass\n"
        } else {
            ""
        };
        let error = support::check_source(&format!(
            "class C:\n    def {method}(self: &mut C) -> Result[(), str]:\n        Ok(())\n{usage}"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("must return ()"), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn arithmetic_constructor_errors_do_not_allocate_or_add_alloc_error() {
    native(
        r#"
class Offset:
    value: i64
    def __init__(self, n: i64) -> Result[(), OverflowError[i64]]:
        self.value = n + 1
        Ok(())
def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    result: Result[Offset, OverflowError[i64]] = Offset(i64.MAX)
    good: Result[Offset, OverflowError[i64]] = Offset.new(0)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    match result:
        case Err(error):
            print(error.wrapped)?
        case Ok(_):
            print("unexpected")?
    print(good.unwrap().value)?
    Ok(())
"#,
        "-9223372036854775808\n1",
    );
}

#[test]
fn constructor_maps_arithmetic_to_its_declared_domain_error() {
    native(
        r#"
enum Problem:
    Extent(OverflowError[i64])
class Offset:
    value: i64
    def __init__(self, n: i64) -> Result[(), Problem]:
        result = checked:
            n + 1
        self.value = result.map_error(Problem.Extent)?
        Ok(())
def main() -> Result[(), IoError]:
    result: Result[Offset, Problem] = Offset.new(i64.MAX)
    match result:
        case Err(Problem.Extent(error)):
            print(error.wrapped)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#,
        "-9223372036854775808",
    );
}

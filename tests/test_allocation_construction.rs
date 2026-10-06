//! Fallible collection construction from an initially empty ownership state.
mod support;
use rstest::rstest;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let visible = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end(), "{source}");
}

#[test]
fn constructors_support_type_aliases_nested_payloads_and_propagation() {
    native(r#"
type Numbers = list[i64]
def numbers() -> Result[Numbers, AllocError]:
    mut values = Numbers.try_with_capacity(2)?
    values.try_append(21)?
    values.try_append(42)?
    Ok(values)
def nested() -> Result[list[list[i64]], AllocError]:
    mut values = list[list[i64]].try_new()?
    values.try_append(numbers()?)?
    Ok(values)
def dictionary() -> Result[dict[str, list[i64]], AllocError]:
    mut values = dict[str, list[i64]].try_with_capacity(1)?
    values.try_insert("answer", numbers()?)?
    Ok(values)
print(numbers())
print(nested())
print(dictionary())
print(set[str].try_new())
print(list[f32].try_new())
"#, "Result[list[i64], AllocError].Ok([21, 42])\nResult[list[list[i64]], AllocError].Ok([[21, 42]])\nResult[dict[str, list[i64]], AllocError].Ok({\"answer\": [21, 42]})\nResult[set[str], AllocError].Ok(set())\nResult[list[f32], AllocError].Ok([])");
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("list[i64]", "values.try_append(n)", 2)]
#[case("set[i64]", "values.try_add(n)", 3)]
#[case("dict[i64, i64]", "values.try_insert(n, n)", 3)]
fn every_constructor_allocation_can_fail_without_leaking(
    #[case] ty: &str,
    #[case] insert: &str,
    #[case] allocations: usize,
) {
    for capacity in [0, 8] {
        let attempts = if capacity == 0 { 1 } else { allocations };
        let constructor = if capacity == 0 {
            "try_new()".to_owned()
        } else {
            format!("try_with_capacity({capacity})")
        };
        for budget in 0..=attempts {
            let source = format!(
                r#"
def build() -> Result[{ty}, AllocError]:
    mut values = {ty}.{constructor}?
    mut n = 0
    while n < {capacity}:
        {insert}?
        n = n + 1
    Ok(values)
print("__test_fail_allocations_after_{budget}__")
result = build()
print("__test_restore_allocations__")
match result:
    case Ok(values):
        print(len(values))
    case Err(error):
        print(error)
# Retry after every failure and success; the first result has left scope.
match build():
    case Ok(values):
        print(len(values))
    case Err(error):
        print("retry failed")
"#
            );
            native(
                &source,
                &format!(
                    "{}\n{capacity}",
                    if budget == attempts {
                        capacity.to_string()
                    } else {
                        "AllocError.OutOfMemory".into()
                    }
                ),
            );
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn invalid_capacities_allocate_nothing_even_when_memory_is_exhausted() {
    native(r#"
print("__test_begin_no_allocations__")
print("__test_fail_allocations_after_0__")
a = list[i64].try_with_capacity(-1)
b = set[i64].try_with_capacity(9223372036854775807)
c = dict[str, i64].try_with_capacity(9223372036854775807)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(a)
print(b)
print(c)
"#, "Result[list[i64], AllocError].Err(AllocError.CapacityOverflow)\nResult[set[i64], AllocError].Err(AllocError.CapacityOverflow)\nResult[dict[str, i64], AllocError].Err(AllocError.CapacityOverflow)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_construction_cleans_up_earlier_arguments_and_skips_later_ones() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped")
def later() -> i64:
    print("unreachable")
    0
def consume(guard: Guard, values: list[i64], after: i64) -> ():
    pass
def build(guard: Guard) -> Result[(), AllocError]:
    consume(guard, list[i64].try_new()?, later())
    Ok(())
guard = Guard()
print("__test_fail_allocations_after_0__")
result = build(guard)
match result:
    case Ok(done):
        print("unexpected success")
    case Err(error):
        print("handled")
print("__test_restore_allocations__")
"#,
        "dropped\nhandled",
    );
}

#[test]
fn capacity_expression_runs_once_and_can_propagate_before_construction() {
    native(r#"
def capacity(ok: bool) -> Result[i64, AllocError]:
    print("capacity")
    Ok(2) if ok else Err(AllocError.CapacityOverflow)
def build(ok: bool) -> Result[list[i64], AllocError]:
    list[i64].try_with_capacity(capacity(ok)?)
print(build(True))
print(build(False))
"#, "capacity\nResult[list[i64], AllocError].Ok([])\ncapacity\nResult[list[i64], AllocError].Err(AllocError.CapacityOverflow)");
}

#[rstest]
#[case("print(list[i64].try_new(1))", "try_new requires 0 argument")]
#[case(
    "print(dict[str, i64].try_with_capacity())",
    "try_with_capacity requires 1 argument"
)]
#[case("print(set[i64].try_with_capacity(1u8))", "expected i64")]
#[case("print(list[i64].try_copy())", "unsupported collection type method")]
#[case("mut values = [1]\nvalues.try_new()", "unsupported method")]
#[case(
    "print(set[list[i64]].try_new())",
    "set elements must be integers, bool, or str"
)]
#[case("values: list[i64] = list[i64].try_new()", "expected list[i64]")]
#[case(
    "type Numbers = list[i64]\nNumbers = [1]\nNumbers.try_new()",
    "shadows a type qualifier"
)]
fn rejects_invalid_constructors(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

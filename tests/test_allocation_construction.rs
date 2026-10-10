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
    mut values = Numbers.with_capacity(2)?
    values.append(21)?
    values.append(42)?
    Ok(values)
def nested() -> Result[list[list[i64]], AllocError]:
    mut values = list[list[i64]].new()?
    values.append(numbers()?)?
    Ok(values)
def dictionary() -> Result[dict[str, list[i64]], AllocError]:
    mut values = dict[str, list[i64]].with_capacity(1)?
    values.insert("answer", numbers()?)?
    Ok(values)
print(numbers()).unwrap()
print(nested()).unwrap()
print(dictionary()).unwrap()
print(set[str].new()).unwrap()
print(list[f32].new()).unwrap()
"#, "Result[list[i64], AllocError].Ok([21, 42])\nResult[list[list[i64]], AllocError].Ok([[21, 42]])\nResult[dict[str, list[i64]], AllocError].Ok({\"answer\": [21, 42]})\nResult[set[str], AllocError].Ok(set())\nResult[list[f32], AllocError].Ok([])");
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("list[i64]", "values.append(n)", 2)]
#[case("set[i64]", "values.add(n)", 3)]
#[case("dict[i64, i64]", "values.insert(n, n)", 3)]
fn every_constructor_allocation_can_fail_without_leaking(
    #[case] ty: &str,
    #[case] insert: &str,
    #[case] allocations: usize,
) {
    for capacity in [0, 8] {
        let attempts = if capacity == 0 { 1 } else { allocations };
        let constructor = if capacity == 0 {
            "new()".to_owned()
        } else {
            format!("with_capacity({capacity})")
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
print("__test_fail_allocations_after_{budget}__").unwrap()
result = build()
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(values):
        print(len(values)).unwrap()
    case Err(error):
        print(error).unwrap()
# Retry after every failure and success; the first result has left scope.
match build():
    case Ok(values):
        print(len(values)).unwrap()
    case Err(error):
        print("retry failed").unwrap()
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
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
a = list[i64].with_capacity(-1)
b = set[i64].with_capacity(9223372036854775807)
c = dict[str, i64].with_capacity(9223372036854775807)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
"#, "Result[list[i64], AllocError].Err(AllocError.CapacityOverflow)\nResult[set[i64], AllocError].Err(AllocError.CapacityOverflow)\nResult[dict[str, i64], AllocError].Err(AllocError.CapacityOverflow)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_construction_cleans_up_earlier_arguments_and_skips_later_ones() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped").unwrap()
def later() -> i64:
    print("unreachable").unwrap()
    0
def consume(guard: Guard, values: list[i64], after: i64) -> ():
    pass
def build(guard: Guard) -> Result[(), AllocError]:
    consume(guard, list[i64].new()?, later())
    Ok(())
guard = Guard()
print("__test_fail_allocations_after_0__").unwrap()
result = build(guard)
match result:
    case Ok(done):
        print("unexpected success").unwrap()
    case Err(error):
        print("handled").unwrap()
print("__test_restore_allocations__").unwrap()
"#,
        "dropped\nhandled",
    );
}

#[test]
fn capacity_expression_runs_once_and_can_propagate_before_construction() {
    native(r#"
def capacity(ok: bool) -> Result[i64, AllocError]:
    print("capacity").unwrap()
    Ok(2) if ok else Err(AllocError.CapacityOverflow)
def build(ok: bool) -> Result[list[i64], AllocError]:
    list[i64].with_capacity(capacity(ok)?)
print(build(True)).unwrap()
print(build(False)).unwrap()
"#, "capacity\nResult[list[i64], AllocError].Ok([])\ncapacity\nResult[list[i64], AllocError].Err(AllocError.CapacityOverflow)");
}

#[rstest]
#[case("print(list[i64].new(1)).unwrap()", "new requires 0 argument")]
#[case(
    "print(dict[str, i64].with_capacity()).unwrap()",
    "with_capacity requires 1 argument"
)]
#[case("print(set[i64].with_capacity(1u8)).unwrap()", "expected i64")]
#[case(
    "print(list[i64].copy()).unwrap()",
    "unsupported collection type method"
)]
#[case("mut values = [1].unwrap()\nvalues.new()", "unsupported method")]
#[case(
    "print(set[list[i64]].new()).unwrap()",
    "set elements must be integers, bool, or str"
)]
#[case("values: list[i64] = list[i64].new()", "expected list[i64]")]
#[case(
    "type Numbers = list[i64]\nNumbers = [1].unwrap()\nNumbers.new()",
    "shadows a type qualifier"
)]
fn rejects_invalid_constructors(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

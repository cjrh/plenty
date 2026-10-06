//! Bulk mutation reserves first and explicitly consumes its source collection.
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
fn extend_moves_elements_in_order_through_exclusive_references_and_fields() {
    native(r#"
def extend(items: &mut list[i64], other: list[i64]) -> Result[(), AllocError]:
    items.try_extend(other)?
    Ok(())
class Data:
    items: list[i64]
mut data = Data([1, 2])
other = [3, 4, 5]
print(extend(&mut data.items, other))
print(data.items.try_extend([]))
print(data.items)
mut nested = [[1]]
print(nested.try_extend([[2], [3]]))
print(nested)
class Custom:
    def try_extend(self, n: i64) -> i64:
        n
print(Custom().try_extend(42))
"#, "Result[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\n[1, 2, 3, 4, 5]\nResult[(), AllocError].Ok(())\n[[1], [2], [3]]\n42");
}

#[test]
fn fallible_copy_preserves_the_source_and_arguments_run_before_mutation() {
    native(
        r#"
def append_copy(items: &mut list[str], source: &list[str]) -> Result[(), AllocError]:
    items.try_extend(try_copy(source)?)
def more(items: &list[i64]) -> list[i64]:
    print(len(items))
    [len(items), len(items) + 1]
mut items = [10]
print(items.try_extend(more(&items)))
print(items)
source = ["A" + "da"]
mut names = ["Bea"]
print(append_copy(&mut names, &source))
drop(names)
print(source)
"#,
        "1\nResult[(), AllocError].Ok(())\n[10, 1, 2]\nResult[(), AllocError].Ok(())\n[\"Ada\"]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn extension_failure_preserves_destination_and_drops_consumed_owners_once() {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print("__test_restore_allocations__")
        print(self.id)
mut destination = list[Guard]()
source = [Guard(1), Guard(2), Guard(3)]
print("__test_fail_allocations_after_{budget}__")
result = destination.try_extend(source)
print("__test_restore_allocations__")
print(result)
print(len(destination))
print("drop destination")
drop(destination)
"#
            ),
            if budget == 0 {
                "1\n2\n3\nResult[(), AllocError].Err(AllocError.OutOfMemory)\n0\ndrop destination"
            } else {
                "Result[(), AllocError].Ok(())\n3\ndrop destination\n1\n2\n3"
            },
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn extension_reuses_reserved_capacity_and_empty_sources_allocate_nothing() {
    native(
        r#"
def run() -> Result[list[str], AllocError]:
    mut items = list[str].try_with_capacity(3)?
    source = ["A" + "da", "B" + "ea"]
    empty = list[str]()
    print("__test_fail_allocations_after_0__")
    print("__test_begin_no_allocations__")
    items.try_extend(source)?
    items.try_extend(empty)?
    print("__test_restore_allocations__")
    print("__test_end_no_allocations__")
    Ok(items)
print(run())
"#,
        "Result[list[str], AllocError].Ok([\"Ada\", \"Bea\"])",
    );
}

#[rstest]
#[case("items = [1]\nitems.try_extend([2])", "immutable")]
#[case(
    "mut items = [1]\nsource = [2]\nitems.try_extend(source)\nprint(source)",
    "moved"
)]
#[case("mut items = [1]\nitems.try_extend(items)", "moved")]
#[case(
    "mut items = [1]\nsource = [2]\nitems.try_extend(&source)",
    "expected list[i64]"
)]
#[case("mut items = [1]\nitems.try_extend([2u8])", "expected list[i64]")]
#[case(
    "mut items = [1]\nitems.try_extend()",
    "try_extend requires 1 argument"
)]
#[case("[1].try_extend([2])", "try_extend requires a mutable list")]
#[case(
    "mut items = [1]\nloan = &items\nitems.try_extend([2])\nprint(loan)",
    "borrow"
)]
fn invalid_extension(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn dictionary_update_preserves_existing_order_and_transfers_nested_values() {
    native(r#"
def update(data: &mut dict[str, i64], source: dict[str, i64]) -> Result[(), AllocError]:
    data.try_update(source)
class Data:
    entries: dict[str, i64]
mut data = Data({"a": 1, "b": 2})
print(update(&mut data.entries, {"b": 20, "c": 30, "a": 10, "d": 40}))
print(data.entries)
print(data.entries.try_update({}))
mut nested = {1: [10]}
print(nested.try_update({1: [20], 2: [30]}))
print(nested)
class Custom:
    def try_update(self, n: i64) -> i64:
        n
print(Custom().try_update(42))
"#, "Result[(), AllocError].Ok(())\n{\"a\": 10, \"b\": 20, \"c\": 30, \"d\": 40}\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\n{1: [20], 2: [30]}\n42");
}

#[test]
fn update_explicit_copy_preserves_sources_and_argument_errors_skip_mutation() {
    native(r#"
def update(data: &mut dict[str, list[i64]], source: &dict[str, list[i64]]) -> Result[(), AllocError]:
    data.try_update(try_copy(source)?)
def missing() -> Result[dict[str, list[i64]], AllocError]:
    Err(AllocError.CapacityOverflow)
def fail(data: &mut dict[str, list[i64]]) -> Result[(), AllocError]:
    data.try_update(missing()?)
mut data = {"a": [1]}
source = {"a": [2], "b": [3]}
print(update(&mut data, &source))
print(fail(&mut data))
print(data)
print(source)
"#, "Result[(), AllocError].Ok(())\nResult[(), AllocError].Err(AllocError.CapacityOverflow)\n{\"a\": [2], \"b\": [3]}\n{\"a\": [2], \"b\": [3]}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn dictionary_update_reserves_both_buffers_before_replacing_any_owner() {
    let entries = (1..=10)
        .map(|n| format!("{n}: Guard({})", 100 + n))
        .collect::<Vec<_>>()
        .join(", ");
    let incoming_drops = (101..=110)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    for budget in 0..=2 {
        native(
            &format!(
                r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print("__test_restore_allocations__")
        print(self.id)
mut data = {{1: Guard(1)}}
source = {{{entries}}}
print("__test_fail_allocations_after_{budget}__")
result = data.try_update(source)
print("__test_restore_allocations__")
print(result)
print(len(data))
print("drop destination")
drop(data)
"#
            ),
            &if budget < 2 {
                format!("{incoming_drops}\nResult[(), AllocError].Err(AllocError.OutOfMemory)\n1\ndrop destination\n1")
            } else {
                format!("1\nResult[(), AllocError].Ok(())\n10\ndrop destination\n{incoming_drops}")
            },
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn replacement_only_and_reserved_dictionary_updates_allocate_nothing() {
    native(
        r#"
def run() -> Result[dict[str, str], AllocError]:
    mut data = dict[str, str].try_with_capacity(3)?
    data.try_insert("a", "old")?
    replacements = {"a": "n" + "ew"}
    additions = {"b": "B" + "ea", "c": "C" + "am"}
    empty = dict[str, str]()
    print("__test_fail_allocations_after_0__")
    print("__test_begin_no_allocations__")
    data.try_update(replacements)?
    data.try_update(additions)?
    data.try_update(empty)?
    print("__test_restore_allocations__")
    print("__test_end_no_allocations__")
    Ok(data)
print(run())
"#,
        "Result[dict[str, str], AllocError].Ok({\"a\": \"new\", \"b\": \"Bea\", \"c\": \"Cam\"})",
    );
}

#[rstest]
#[case("data = {1: 2}\ndata.try_update({1: 3})", "immutable")]
#[case(
    "mut data = {1: 2}\nsource = {2: 3}\ndata.try_update(source)\nprint(source)",
    "moved"
)]
#[case("mut data = {1: 2}\ndata.try_update(data)", "moved")]
#[case(
    "mut data = {1: 2}\nsource = {2: 3}\ndata.try_update(&source)",
    "expected dict[i64, i64]"
)]
#[case(
    "mut data = {1: 2}\ndata.try_update({1u8: 3})",
    "expected dict[i64, i64]"
)]
#[case(
    "mut data = {1: 2}\ndata.try_update()",
    "try_update requires 1 argument"
)]
#[case(
    "{1: 2}.try_update({2: 3})",
    "try_update requires a mutable dictionary"
)]
#[case(
    "mut data = {1: 2}\nloan = &data\ndata.try_update({2: 3})\nprint(loan)",
    "borrow"
)]
fn invalid_dictionary_update(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

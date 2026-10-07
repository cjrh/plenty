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
    items.extend(other)?
    Ok(())
class Data:
    items: list[i64]
mut data = Data([1, 2].unwrap()).unwrap()
other = [3, 4, 5].unwrap()
print(extend(&mut data.items, other)).unwrap()
print(data.items.extend([].unwrap())).unwrap()
print(data.items).unwrap()
mut nested = [[1].unwrap()].unwrap()
print(nested.extend([[2].unwrap(), [3].unwrap()].unwrap())).unwrap()
print(nested).unwrap()
class Custom:
    def extend(self, n: i64) -> i64:
        n
print(Custom().unwrap().extend(42)).unwrap()
"#, "Result[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\n[1, 2, 3, 4, 5]\nResult[(), AllocError].Ok(())\n[[1], [2], [3]]\n42");
}

#[test]
fn fallible_copy_preserves_the_source_and_arguments_run_before_mutation() {
    native(
        r#"
def append_copy(items: &mut list[str], source: &list[str]) -> Result[(), AllocError]:
    items.extend(copy(source)?)
def more(items: &list[i64]) -> list[i64]:
    print(len(items)).unwrap()
    [len(items), len(items) + 1].unwrap()
mut items = [10].unwrap()
print(items.extend(more(&items))).unwrap()
print(items).unwrap()
source = [("A" + "da").unwrap()].unwrap()
mut names = ["Bea"].unwrap()
print(append_copy(&mut names, &source)).unwrap()
drop(names)
print(source).unwrap()
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
        print("__test_restore_allocations__").unwrap()
        print(self.id).unwrap()
mut destination = list[Guard]().unwrap()
source = [Guard(1).unwrap(), Guard(2).unwrap(), Guard(3).unwrap()].unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = destination.extend(source)
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(len(destination)).unwrap()
print("drop destination").unwrap()
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
    mut items = list[str].with_capacity(3)?
    source = [("A" + "da").unwrap(), ("B" + "ea").unwrap()].unwrap()
    empty = list[str]().unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    print("__test_begin_no_allocations__").unwrap()
    items.extend(source)?
    items.extend(empty)?
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    Ok(items)
print(run()).unwrap()
"#,
        "Result[list[str], AllocError].Ok([\"Ada\", \"Bea\"])",
    );
}

#[rstest]
#[case("items = [1].unwrap()\nitems.extend([2].unwrap())", "immutable")]
#[case(
    "mut items = [1].unwrap()\nsource = [2].unwrap()\nitems.extend(source)\nprint(source).unwrap()",
    "moved"
)]
#[case("mut items = [1].unwrap()\nitems.extend(items)", "moved")]
#[case(
    "mut items = [1].unwrap()\nsource = [2].unwrap()\nitems.extend(&source)",
    "expected list[i64]"
)]
#[case(
    "mut items = [1].unwrap()\nitems.extend([2u8].unwrap())",
    "expected list[i64]"
)]
#[case(
    "mut items = [1].unwrap()\nitems.extend()",
    "extend requires 1 argument"
)]
#[case("[1].unwrap().extend([2].unwrap())", "extend requires a mutable list")]
#[case(
    "mut items = [1].unwrap()\nloan = &items\nitems.extend([2].unwrap())\nprint(loan).unwrap()",
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
    data.update(source)
class Data:
    entries: dict[str, i64]
mut data = Data({"a": 1, "b": 2}.unwrap()).unwrap()
print(update(&mut data.entries, {"b": 20, "c": 30, "a": 10, "d": 40}.unwrap())).unwrap()
print(data.entries).unwrap()
print(data.entries.update({}.unwrap())).unwrap()
mut nested = {1: [10].unwrap()}.unwrap()
print(nested.update({1: [20].unwrap(), 2: [30].unwrap()}.unwrap())).unwrap()
print(nested).unwrap()
class Custom:
    def update(self, n: i64) -> i64:
        n
print(Custom().unwrap().update(42)).unwrap()
"#, "Result[(), AllocError].Ok(())\n{\"a\": 10, \"b\": 20, \"c\": 30, \"d\": 40}\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\n{1: [20], 2: [30]}\n42");
}

#[test]
fn update_explicit_copy_preserves_sources_and_argument_errors_skip_mutation() {
    native(r#"
def update(data: &mut dict[str, list[i64]], source: &dict[str, list[i64]]) -> Result[(), AllocError]:
    data.update(copy(source)?)
def missing() -> Result[dict[str, list[i64]], AllocError]:
    Err(AllocError.CapacityOverflow)
def fail(data: &mut dict[str, list[i64]]) -> Result[(), AllocError]:
    data.update(missing()?)
mut data = {"a": [1].unwrap()}.unwrap()
source = {"a": [2].unwrap(), "b": [3].unwrap()}.unwrap()
print(update(&mut data, &source)).unwrap()
print(fail(&mut data)).unwrap()
print(data).unwrap()
print(source).unwrap()
"#, "Result[(), AllocError].Ok(())\nResult[(), AllocError].Err(AllocError.CapacityOverflow)\n{\"a\": [2], \"b\": [3]}\n{\"a\": [2], \"b\": [3]}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn dictionary_update_reserves_both_buffers_before_replacing_any_owner() {
    let entries = (1..=10)
        .map(|n| format!("{n}: Guard({}).unwrap()", 100 + n))
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
        print("__test_restore_allocations__").unwrap()
        print(self.id).unwrap()
mut data = {{1: Guard(1).unwrap()}}.unwrap()
source = {{{entries}}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = data.update(source)
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(len(data)).unwrap()
print("drop destination").unwrap()
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
    mut data = dict[str, str].with_capacity(3)?
    data.insert("a", "old")?
    replacements = {"a": ("n" + "ew").unwrap()}.unwrap()
    additions = {"b": ("B" + "ea").unwrap(), "c": ("C" + "am").unwrap()}.unwrap()
    empty = dict[str, str]().unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    print("__test_begin_no_allocations__").unwrap()
    data.update(replacements)?
    data.update(additions)?
    data.update(empty)?
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    Ok(data)
print(run()).unwrap()
"#,
        "Result[dict[str, str], AllocError].Ok({\"a\": \"new\", \"b\": \"Bea\", \"c\": \"Cam\"})",
    );
}

#[rstest]
#[case("data = {1: 2}.unwrap()\ndata.update({1: 3}.unwrap())", "immutable")]
#[case(
    "mut data = {1: 2}.unwrap()\nsource = {2: 3}.unwrap()\ndata.update(source)\nprint(source).unwrap()",
    "moved"
)]
#[case("mut data = {1: 2}.unwrap()\ndata.update(data)", "moved")]
#[case(
    "mut data = {1: 2}.unwrap()\nsource = {2: 3}.unwrap()\ndata.update(&source)",
    "expected dict[i64, i64]"
)]
#[case(
    "mut data = {1: 2}.unwrap()\ndata.update({1u8: 3}.unwrap())",
    "expected dict[i64, i64]"
)]
#[case(
    "mut data = {1: 2}.unwrap()\ndata.update()",
    "update requires 1 argument"
)]
#[case(
    "{1: 2}.unwrap().update({2: 3}.unwrap())",
    "update requires a mutable dictionary"
)]
#[case(
    "mut data = {1: 2}.unwrap()\nloan = &data\ndata.update({2: 3}.unwrap())\nprint(loan).unwrap()",
    "borrow"
)]
fn invalid_dictionary_update(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn set_update_transfers_members_and_eliminates_content_equal_duplicates() {
    native(r#"
def update(data: &mut set[str], source: set[str]) -> Result[(), AllocError]:
    data.update(source)
class Data:
    members: set[str]
mut data = Data({("é" + "🙂").unwrap()}.unwrap()).unwrap()
print(update(&mut data.members, {("" + "é🙂").unwrap(), "other"}.unwrap())).unwrap()
print(len(data.members)).unwrap()
print("é🙂" in data.members).unwrap()
print("other" in data.members).unwrap()
print(data.members.update(set[str]().unwrap())).unwrap()
mut flags = {False}.unwrap()
print(flags.update({False, True}.unwrap())).unwrap()
print(len(flags)).unwrap()
mut bytes = {1u8}.unwrap()
print(bytes.update({1u8, 2u8}.unwrap())).unwrap()
print(2u8 in bytes).unwrap()
"#, "Result[(), AllocError].Ok(())\n2\nTrue\nTrue\nResult[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\n2\nResult[(), AllocError].Ok(())\nTrue");
}

#[test]
fn set_update_keeps_probe_chains_valid_through_growth_and_removal() {
    native(
        r#"
mut values = {n for n in range(100) if n % 2 == 0}.unwrap()
print(values.update({n for n in range(100)}.unwrap())).unwrap()
mut valid = len(values) == 100
for n in range(100):
    valid = valid and n in values
for n in range(25):
    values.discard(n)
print(values.update({n for n in range(25)}.unwrap())).unwrap()
for n in range(100):
    valid = valid and n in values
print(valid).unwrap()
print(len(values)).unwrap()
"#,
        "Result[(), AllocError].Ok(())\nResult[(), AllocError].Ok(())\nTrue\n100",
    );
}

#[test]
fn set_update_can_explicitly_copy_a_source_before_consuming_it() {
    native(
        r#"
def update(data: &mut set[str], source: &set[str]) -> Result[(), AllocError]:
    data.update(copy(source)?)
mut data = {"Ada"}.unwrap()
source = {("A" + "da").unwrap(), ("B" + "ea").unwrap()}.unwrap()
print(update(&mut data, &source)).unwrap()
drop(data)
print(len(source)).unwrap()
print("Bea" in source).unwrap()
"#,
        "Result[(), AllocError].Ok(())\n2\nTrue",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn set_update_failure_preserves_contents_and_the_hash_index() {
    for budget in 0..=2 {
        native(
            &format!(
                r#"
mut data = {{0}}.unwrap()
source = {{n for n in range(11)}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = data.update(source)
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(len(data)).unwrap()
print(0 in data).unwrap()
print(10 in data).unwrap()
data.add(42).unwrap()
print(42 in data).unwrap()
"#
            ),
            if budget < 2 {
                "Result[(), AllocError].Err(AllocError.OutOfMemory)\n1\nTrue\nFalse\nTrue"
            } else {
                "Result[(), AllocError].Ok(())\n11\nTrue\nTrue\nTrue"
            },
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn set_update_uses_existing_storage_and_cleans_duplicate_string_owners() {
    native(
        r#"
def run() -> Result[set[str], AllocError]:
    mut data = set[str].with_capacity(3)?
    data.add(("A" + "da").unwrap())?
    duplicates = {("Ad" + "a").unwrap()}.unwrap()
    additions = {("B" + "ea").unwrap(), ("C" + "am").unwrap()}.unwrap()
    empty = set[str]().unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    print("__test_begin_no_allocations__").unwrap()
    data.update(duplicates)?
    data.update(additions)?
    data.update(empty)?
    print("__test_restore_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
    Ok(data)
match run():
    case Ok(data):
        print(len(data)).unwrap()
        print("Ada" in data and "Bea" in data and "Cam" in data).unwrap()
    case Err(error):
        print(error).unwrap()
"#,
        "3\nTrue",
    );
}

#[rstest]
#[case("data = {1}.unwrap()\ndata.update({2}.unwrap())", "immutable")]
#[case(
    "mut data = {1}.unwrap()\nsource = {2}.unwrap()\ndata.update(source)\nprint(source).unwrap()",
    "moved"
)]
#[case("mut data = {1}.unwrap()\ndata.update(data)", "moved")]
#[case(
    "mut data = {1}.unwrap()\nsource = {2}.unwrap()\ndata.update(&source)",
    "expected set[i64]"
)]
#[case(
    "mut data = {1}.unwrap()\ndata.update([2].unwrap())",
    "collection type does not match its annotation"
)]
#[case(
    "mut data = {1}.unwrap()\ndata.update({2u8}.unwrap())",
    "expected set[i64]"
)]
#[case("mut data = {1}.unwrap()\ndata.update()", "update requires 1 argument")]
#[case(
    "{1}.unwrap().update({2}.unwrap())",
    "update requires a mutable dictionary or set"
)]
#[case(
    "mut data = {1}.unwrap()\nloan = &data\ndata.update({2}.unwrap())\nprint(loan).unwrap()",
    "borrow"
)]
fn invalid_set_update(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

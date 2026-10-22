//! Fallible dictionary snapshots preserve order, ownership, and failure cleanup.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn snapshots_preserve_order_types_and_empty_lists() {
    native(
        r#"
mut data = {3u8: 2.5f32, 1u8: -1.5f32, 2u8: 0f32}.unwrap()
data[1u8] = 4.5f32
print(str.repr(data.keys_snapshot()).unwrap()).unwrap()
print(str.repr(data.values_snapshot()).unwrap()).unwrap()
print(data).unwrap()
print(str.repr(dict[str, i64]().unwrap().keys_snapshot()).unwrap()).unwrap()
print(str.repr(dict[str, i64]().unwrap().values_snapshot()).unwrap()).unwrap()
flags = {False: True, True: False}.unwrap()
print(str.repr(flags.keys_snapshot()).unwrap()).unwrap()
print(str.repr(flags.values_snapshot()).unwrap()).unwrap()
"#,
        "Result[list[u8], AllocError].Ok([3, 1, 2])\nResult[list[f32], AllocError].Ok([2.5, 4.5, 0.0])\n{3: 2.5, 1: 4.5, 2: 0.0}\nResult[list[str], AllocError].Ok([])\nResult[list[i64], AllocError].Ok([])\nResult[list[bool], AllocError].Ok([False, True])\nResult[list[bool], AllocError].Ok([True, False])",
    );
}

#[test]
fn snapshots_retain_strings_and_inline_and_heap_enum_payloads() {
    native(
        r#"
enum Label:
    Text(str)
mut data = {("na" + "me").unwrap(): ("é" + "🙂").unwrap()}.unwrap()
keys = data.keys_snapshot()
values = data.values_snapshot()
data["name"] = "changed"
drop(data)
print(str.repr(keys).unwrap()).unwrap()
print(str.repr(values).unwrap()).unwrap()
print(str.repr({0: Label.Text(("A" + "da").unwrap())}.unwrap().values_snapshot()).unwrap()).unwrap()
options: dict[i64, Option[str]] = {0: Nothing, 1: Some(("B" + "ea").unwrap())}.unwrap()
result = options.values_snapshot()
drop(options)
print(str.repr(result).unwrap()).unwrap()
"#,
        "Result[list[str], AllocError].Ok([\"name\"])\nResult[list[str], AllocError].Ok([\"é🙂\"])\nResult[list[Label], AllocError].Ok([Label.Text(\"Ada\")])\nResult[list[Option[str]], AllocError].Ok([Option[str].Nothing, Option[str].Some(\"Bea\")])",
    );
}

#[test]
fn references_fields_and_propagation_preserve_the_source() {
    native(
        r#"
class Data:
    entries: dict[str, i64]
def values(data: &dict[str, i64]) -> Result[list[i64], AllocError]:
    items = data.values_snapshot()?
    Ok(items)
def keys(data: &mut dict[str, i64]) -> Result[list[str], AllocError]:
    data.keys_snapshot()
mut data = Data({"a": 1, "b": 2}.unwrap())
print(str.repr(values(&data.entries)).unwrap()).unwrap()
print(str.repr(keys(&mut data.entries)).unwrap()).unwrap()
print(str.repr(data.entries.values_snapshot()).unwrap()).unwrap()
data.entries["a"] = 3
print(data.entries).unwrap()
"#,
        "Result[list[i64], AllocError].Ok([1, 2])\nResult[list[str], AllocError].Ok([\"a\", \"b\"])\nResult[list[i64], AllocError].Ok([1, 2])\n{\"a\": 3, \"b\": 2}",
    );
}

#[test]
fn explicit_fallible_copy_produces_independent_mutable_values() {
    native(
        r#"
def snapshot(data: &dict[str, list[i64]]) -> Result[list[list[i64]], AllocError]:
    mut items = copy(data)?.values_snapshot()?
    match items.pop():
        case Some(item):
            mut changed = item
            changed.append(3).unwrap()
            items.append(changed)?
        case Nothing:
            pass
    Ok(items)
data = {"a": [1, 2].unwrap()}.unwrap()
print(str.repr(snapshot(&data)).unwrap()).unwrap()
print(data).unwrap()
print(str.repr(data.keys_snapshot()).unwrap()).unwrap()
"#,
        "Result[list[list[i64]], AllocError].Ok([[1, 2, 3]])\n{\"a\": [1, 2]}\nResult[list[str], AllocError].Ok([\"a\"])",
    );
}

#[test]
fn owned_temporary_values_transfer_custom_cleanup_without_early_drop() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def make() -> dict[i64, Guard]:
    print("make").unwrap()
    {0: Guard(10), 1: Guard(20)}.unwrap()
result = make().values_snapshot()
print("snapshot").unwrap()
drop(result)
print("done").unwrap()
"#,
        "make\nsnapshot\n10\n20\ndone",
    );
}

#[test]
fn receiver_runs_once_and_inherent_methods_keep_their_names() {
    native(
        r#"
def make() -> dict[i64, i64]:
    print("make").unwrap()
    {1: 2}.unwrap()
class Custom:
    def keys(self, n: i64) -> i64:
        n
    def values(self) -> bool:
        True
print(str.repr(make().keys_snapshot()).unwrap()).unwrap()
print(str.repr(make().values_snapshot()).unwrap()).unwrap()
print(Custom().keys(42)).unwrap()
custom = Custom()
print(custom.values()).unwrap()
"#,
        "make\nResult[list[i64], AllocError].Ok([1])\nmake\nResult[list[i64], AllocError].Ok([2])\n42\nTrue",
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("keys_snapshot", "str", "[\"key\"]")]
#[case("values_snapshot", "str", "[\"value\"]")]
fn each_snapshot_allocation_can_fail_and_retry(
    #[case] method: &str,
    #[case] element: &str,
    #[case] expected: &str,
) {
    for budget in 0..=2 {
        native(
            &format!(
                r#"
data = {{("k" + "ey").unwrap(): ("val" + "ue").unwrap()}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = data.{method}()
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(data).unwrap()
print(str.repr(data.{method}()).unwrap()).unwrap()
"#
            ),
            &format!(
                "Result[list[{element}], AllocError].{}\n{{\"key\": \"value\"}}\nResult[list[{element}], AllocError].Ok({expected})",
                if budget < 2 {
                    "Err(AllocError.OutOfMemory)".to_owned()
                } else {
                    format!("Ok({expected})")
                }
            ),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn empty_snapshots_need_only_the_list_header() {
    for method in ["keys_snapshot", "values_snapshot"] {
        for budget in 0..=1 {
            native(
                &format!(
                    r#"
data = dict[i64, i64]().unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = data.{method}()
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(data).unwrap()
"#
                ),
                &format!(
                    "Result[list[i64], AllocError].{}\n{{}}",
                    if budget == 0 {
                        "Err(AllocError.OutOfMemory)"
                    } else {
                        "Ok([])"
                    }
                ),
            );
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_snapshot_propagation_cleans_consumed_temporary_payloads_once() {
    for budget in 0..=2 {
        native(
            &format!(
                r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print("__test_restore_allocations__").unwrap()
        print(self.id).unwrap()
def consume(data: dict[i64, Guard]) -> dict[i64, Guard]:
    data
def snapshot(data: dict[i64, Guard]) -> Result[list[Guard], AllocError]:
    print("__test_fail_allocations_after_{budget}__").unwrap()
    result = consume(data).values_snapshot()?
    print("__test_restore_allocations__").unwrap()
    print("success").unwrap()
    Ok(result)
result = snapshot({{0: Guard(10), 1: Guard(20)}}.unwrap())
print("returned").unwrap()
drop(result)
"#
            ),
            if budget < 2 {
                "10\n20\nreturned"
            } else {
                "success\nreturned\n10\n20"
            },
        );
    }
}

#[rstest]
#[case(
    "print({1: 2}.unwrap().keys_snapshot(0)).unwrap()",
    "keys_snapshot takes no arguments"
)]
#[case(
    "print({1: 2}.unwrap().values_snapshot(0)).unwrap()",
    "values_snapshot takes no arguments"
)]
#[case(
    "print([1].unwrap().keys_snapshot()).unwrap()",
    "unsupported method `keys_snapshot`"
)]
#[case(
    "print(\"text\".values_snapshot()).unwrap()",
    "unsupported method `values_snapshot`"
)]
#[case(
    "data = {1: [2].unwrap()}.unwrap()\nprint(data.values_snapshot()).unwrap()",
    "values_snapshot with owned payloads requires an owned temporary"
)]
#[case(
    "data = {1: [2].unwrap()}.unwrap()\nloan = &data\nprint(loan.values_snapshot()).unwrap()",
    "values_snapshot with owned payloads requires an owned temporary"
)]
#[case(
    "mut data = {1: [2].unwrap()}.unwrap()\nloan = &mut data\nprint(loan.values_snapshot()).unwrap()",
    "values_snapshot with owned payloads requires an owned temporary"
)]
#[case(
    "mut data = {1: 2}.unwrap()\nloan = &mut data\nprint(str.repr(data.keys_snapshot()).unwrap()).unwrap()\nloan[1] = 3",
    "borrow"
)]
fn rejects_invalid_snapshots(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

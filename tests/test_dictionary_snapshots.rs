//! Fallible dictionary snapshots preserve order, ownership, and failure cleanup.
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
fn snapshots_preserve_order_types_and_empty_lists() {
    native(r#"
mut data = {3u8: 2.5f32, 1u8: -1.5f32, 2u8: 0f32}
data[1u8] = 4.5f32
print(data.try_keys())
print(data.try_values())
print(data)
print(dict[str, i64]().try_keys())
print(dict[str, i64]().try_values())
flags = {False: True, True: False}
print(flags.try_keys())
print(flags.try_values())
"#, "Result[list[u8], AllocError].Ok([3, 1, 2])\nResult[list[f32], AllocError].Ok([2.5, 4.5, 0.0])\n{3: 2.5, 1: 4.5, 2: 0.0}\nResult[list[str], AllocError].Ok([])\nResult[list[i64], AllocError].Ok([])\nResult[list[bool], AllocError].Ok([False, True])\nResult[list[bool], AllocError].Ok([True, False])");
}

#[test]
fn snapshots_retain_strings_and_inline_and_heap_enum_payloads() {
    native(r#"
enum Label:
    Text(str)
mut data = {"na" + "me": "é" + "🙂"}
keys = data.try_keys()
values = data.try_values()
data["name"] = "changed"
drop(data)
print(keys)
print(values)
print({0: Label.Text("A" + "da")}.try_values())
options: dict[i64, Option[str]] = {0: Nothing, 1: Some("B" + "ea")}
result = options.try_values()
drop(options)
print(result)
"#, "Result[list[str], AllocError].Ok([\"name\"])\nResult[list[str], AllocError].Ok([\"é🙂\"])\nResult[list[Label], AllocError].Ok([Label.Text(\"Ada\")])\nResult[list[Option[str]], AllocError].Ok([Option[str].Nothing, Option[str].Some(\"Bea\")])");
}

#[test]
fn references_fields_and_propagation_preserve_the_source() {
    native(r#"
class Data:
    entries: dict[str, i64]
def values(data: &dict[str, i64]) -> Result[list[i64], AllocError]:
    items = data.try_values()?
    Ok(items)
def keys(data: &mut dict[str, i64]) -> Result[list[str], AllocError]:
    data.try_keys()
mut data = Data({"a": 1, "b": 2})
print(values(&data.entries))
print(keys(&mut data.entries))
print(data.entries.try_values())
data.entries["a"] = 3
print(data.entries)
"#, "Result[list[i64], AllocError].Ok([1, 2])\nResult[list[str], AllocError].Ok([\"a\", \"b\"])\nResult[list[i64], AllocError].Ok([1, 2])\n{\"a\": 3, \"b\": 2}");
}

#[test]
fn explicit_fallible_copy_produces_independent_mutable_values() {
    native(r#"
def snapshot(data: &dict[str, list[i64]]) -> Result[list[list[i64]], AllocError]:
    mut items = try_copy(data)?.try_values()?
    match items.pop():
        case Some(item):
            mut changed = item
            changed.append(3)
            items.try_append(changed)?
        case Nothing:
            pass
    Ok(items)
data = {"a": [1, 2]}
print(snapshot(&data))
print(data)
print(data.try_keys())
"#, "Result[list[list[i64]], AllocError].Ok([[1, 2, 3]])\n{\"a\": [1, 2]}\nResult[list[str], AllocError].Ok([\"a\"])");
}

#[test]
fn owned_temporary_values_transfer_custom_cleanup_without_early_drop() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self) -> ():
        print(self.id)
def make() -> dict[i64, Guard]:
    print("make")
    {0: Guard(10), 1: Guard(20)}
result = make().try_values()
print("snapshot")
drop(result)
print("done")
"#,
        "make\nsnapshot\n10\n20\ndone",
    );
}

#[test]
fn receiver_runs_once_and_inherent_methods_keep_their_names() {
    native(r#"
def make() -> dict[i64, i64]:
    print("make")
    {1: 2}
class Custom:
    def try_keys(self, n: i64) -> i64:
        n
    def try_values(self) -> bool:
        True
print(make().try_keys())
print(make().try_values())
print(Custom().try_keys(42))
custom = Custom()
print(custom.try_values())
"#, "make\nResult[list[i64], AllocError].Ok([1])\nmake\nResult[list[i64], AllocError].Ok([2])\n42\nTrue");
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("try_keys", "str", "[\"key\"]")]
#[case("try_values", "str", "[\"value\"]")]
fn each_snapshot_allocation_can_fail_and_retry(
    #[case] method: &str,
    #[case] element: &str,
    #[case] expected: &str,
) {
    for budget in 0..=2 {
        native(&format!(r#"
data = {{"k" + "ey": "val" + "ue"}}
print("__test_fail_allocations_after_{budget}__")
result = data.{method}()
print("__test_restore_allocations__")
print(result)
print(data)
print(data.{method}())
"#), &format!("Result[list[{element}], AllocError].{}\n{{\"key\": \"value\"}}\nResult[list[{element}], AllocError].Ok({expected})", if budget < 2 { "Err(AllocError.OutOfMemory)".to_owned() } else { format!("Ok({expected})") }));
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn empty_snapshots_need_only_the_list_header() {
    for method in ["try_keys", "try_values"] {
        for budget in 0..=1 {
            native(
                &format!(
                    r#"
data = dict[i64, i64]()
print("__test_fail_allocations_after_{budget}__")
result = data.{method}()
print("__test_restore_allocations__")
print(result)
print(data)
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
        print("__test_restore_allocations__")
        print(self.id)
def consume(data: dict[i64, Guard]) -> dict[i64, Guard]:
    data
def snapshot(data: dict[i64, Guard]) -> Result[list[Guard], AllocError]:
    print("__test_fail_allocations_after_{budget}__")
    result = consume(data).try_values()?
    print("__test_restore_allocations__")
    print("success")
    Ok(result)
result = snapshot({{0: Guard(10), 1: Guard(20)}})
print("returned")
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
#[case("print({1: 2}.try_keys(0))", "try_keys takes no arguments")]
#[case("print({1: 2}.try_values(0))", "try_values takes no arguments")]
#[case("print([1].try_keys())", "unsupported method `try_keys`")]
#[case("print(\"text\".try_values())", "unsupported method `try_values`")]
#[case(
    "data = {1: [2]}\nprint(data.try_values())",
    "try_values with owned payloads requires an owned temporary"
)]
#[case(
    "data = {1: [2]}\nloan = &data\nprint(loan.try_values())",
    "try_values with owned payloads requires an owned temporary"
)]
#[case(
    "mut data = {1: [2]}\nloan = &mut data\nprint(loan.try_values())",
    "try_values with owned payloads requires an owned temporary"
)]
#[case(
    "mut data = {1: 2}\nloan = &mut data\nprint(data.try_keys())\nloan[1] = 3",
    "borrow"
)]
fn rejects_invalid_snapshots(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

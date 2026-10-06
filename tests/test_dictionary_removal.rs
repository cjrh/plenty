//! Dictionary removal transfers ownership and preserves ordered hash storage.
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
fn pop_transfers_lists_and_preserves_order_on_reinsertion() {
    native(r#"
mut values = {"a": [1], "b": [2], "c": [3]}
match values.pop("b"):
    case Some(items):
        mut changed = items
        changed.append(4)
        values["b"] = changed
    case Nothing:
        print("missing")
print(values)
print(values.pop("absent"))
print(values.pop("a"))
print(values.pop("b"))
print(values.pop("c"))
print(values.pop("c"))
print(len(values))
"#, "{\"a\": [1], \"c\": [3], \"b\": [2, 4]}\nOption[list[i64]].Nothing\nOption[list[i64]].Some([1])\nOption[list[i64]].Some([2, 4])\nOption[list[i64]].Some([3])\nOption[list[i64]].Nothing\n0");
}

#[test]
fn removed_classes_drop_exactly_once_after_their_new_owner() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self: &mut Guard) -> ():
        print(self.id)
mut values = {1: Guard(10), 2: Guard(20), 3: Guard(30)}
removed = values.pop(2)
print("removed")
drop(values)
print("dictionary dropped")
drop(removed)
mut discarded = {1: Guard(40)}
discarded.pop(1)
print("done")
"#,
        "removed\n10\n30\ndictionary dropped\n20\n40\ndone",
    );
}

#[test]
fn borrowed_fields_keys_and_key_expression_are_supported() {
    native(r#"
class Storage:
    values: dict[str, str]
def take(values: &mut dict[str, str], key: &str) -> Option[str]:
    values.pop(key)
def key() -> str:
    print("key")
    "a"
class Custom:
    def pop(self, n: i64) -> i64:
        n
mut storage = Storage({"a": "b", "b": "value"})
print(storage.values.pop(storage.values["a"]))
print(storage.values.pop(key()))
name = "missing"
print(take(&mut storage.values, &name))
print(name)
print(Custom().pop(42))
"#, "Option[str].Some(\"value\")\nkey\nOption[str].Some(\"b\")\nOption[str].Nothing\nmissing\n42");
}

#[test]
fn nested_owned_sums_and_option_propagation_preserve_ownership() {
    native(
        r#"
def take(values: &mut dict[i64, Option[list[i64]]], key: i64) -> Option[list[i64]]:
    values.pop(key)?
mut values: dict[i64, Option[list[i64]]] = {1: Nothing, 2: Some([42])}
print(take(&mut values, 1))
print(take(&mut values, 2))
print(take(&mut values, 3))
print(len(values))
"#,
        "Option[list[i64]].Nothing\nOption[list[i64]].Some([42])\nOption[list[i64]].Nothing\n0",
    );
}

#[test]
fn repeated_removal_rebuilds_probe_chains_and_preserves_remaining_order() {
    native(
        r#"
mut values = dict[i64, i64]()
for n in range(100):
    values[n] = n * 2
mut valid = True
for n in range(0, 100, 2):
    match values.pop(n):
        case Some(value):
            valid = valid and value == n * 2
        case Nothing:
            valid = False
for n in range(1, 100, 2):
    match values.get(n):
        case Some(value):
            valid = valid and value == n * 2
        case Nothing:
            valid = False
print(valid)
print(values.keys() == [n for n in range(1, 100, 2)])
for n in range(0, 100, 2):
    values[n] = n
print(len(values))
print(values.pop(0))
print(values.pop(99))
"#,
        "True\nTrue\n100\nOption[i64].Some(0)\nOption[i64].Some(198)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn removal_and_reinsertion_reuse_capacity_with_allocation_disabled() {
    native(
        r#"
mut values = {"a": [1], "b": [2], "c": [3]}
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
missing = values.pop("absent")
removed = values.pop("b")
match removed:
    case Some(items):
        values["d"] = items
    case Nothing:
        print("unexpected")
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(missing)
print(values)
"#,
        "Option[list[i64]].Nothing\n{\"a\": [1], \"c\": [3], \"d\": [2]}",
    );
}

#[rstest]
#[case("data = {1: 2}\nprint(data.pop(1))", "mutable")]
#[case(
    "mut data = {1: 2}\nprint(data.pop())",
    "pop requires one key argument"
)]
#[case(
    "mut data = {1: 2}\nprint(data.pop(1, 0))",
    "pop requires one key argument"
)]
#[case("mut data = {1u8: 2}\nprint(data.pop(1))", "expected u8")]
#[case("print({1: 2}.pop(1))", "pop requires a mutable list or dictionary")]
#[case(
    "mut data = {1}\nprint(data.pop(0))",
    "pop requires a mutable list or dictionary"
)]
#[case(
    "mut data = {1: [2]}\nloan = &data\nremoved = data.pop(1)\nprint(loan)",
    "borrow"
)]
#[case(
    "mut data = {1: [2]}\nremoved = data.pop(1)\ndrop(removed)\nprint(removed)",
    "moved"
)]
fn rejects_invalid_removal(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

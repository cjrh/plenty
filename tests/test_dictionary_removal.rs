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
mut values = {"a": [1].unwrap(), "b": [2].unwrap(), "c": [3].unwrap()}.unwrap()
match values.pop("b"):
    case Some(items):
        mut changed = items
        changed.append(4).unwrap()
        values.insert("b", changed).unwrap()
    case Nothing:
        print("missing").unwrap()
print(values).unwrap()
print(values.pop("absent")).unwrap()
print(values.pop("a")).unwrap()
print(values.pop("b")).unwrap()
print(values.pop("c")).unwrap()
print(values.pop("c")).unwrap()
print(len(values)).unwrap()
"#, "{\"a\": [1], \"c\": [3], \"b\": [2, 4]}\nOption[list[i64]].Nothing\nOption[list[i64]].Some([1])\nOption[list[i64]].Some([2, 4])\nOption[list[i64]].Some([3])\nOption[list[i64]].Nothing\n0");
}

#[test]
fn removed_classes_drop_exactly_once_after_their_new_owner() {
    native(
        r#"
class Guard:
    id: i64
    def __del__(self: &mut Guard) -> ():
        print(self.id).unwrap()
mut values = {1: Guard(10).unwrap(), 2: Guard(20).unwrap(), 3: Guard(30).unwrap()}.unwrap()
removed = values.pop(2)
print("removed").unwrap()
drop(values)
print("dictionary dropped").unwrap()
drop(removed)
mut discarded = {1: Guard(40).unwrap()}.unwrap()
discarded.pop(1)
print("done").unwrap()
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
    print("key").unwrap()
    "a"
class Custom:
    def pop(self, n: i64) -> i64:
        n
mut storage = Storage({"a": "b", "b": "value"}.unwrap()).unwrap()
print(storage.values.pop(storage.values["a"])).unwrap()
print(storage.values.pop(key())).unwrap()
name = "missing"
print(take(&mut storage.values, &name)).unwrap()
print(name).unwrap()
print(Custom().unwrap().pop(42)).unwrap()
"#, "Option[str].Some(\"value\")\nkey\nOption[str].Some(\"b\")\nOption[str].Nothing\nmissing\n42");
}

#[test]
fn nested_owned_sums_and_option_propagation_preserve_ownership() {
    native(
        r#"
def take(values: &mut dict[i64, Option[list[i64]]], key: i64) -> Option[list[i64]]:
    values.pop(key)?
mut values: dict[i64, Option[list[i64]]] = {1: Nothing, 2: Some([42].unwrap())}.unwrap()
print(take(&mut values, 1)).unwrap()
print(take(&mut values, 2)).unwrap()
print(take(&mut values, 3)).unwrap()
print(len(values)).unwrap()
"#,
        "Option[list[i64]].Nothing\nOption[list[i64]].Some([42])\nOption[list[i64]].Nothing\n0",
    );
}

#[test]
fn repeated_removal_rebuilds_probe_chains_and_preserves_remaining_order() {
    native(
        r#"
mut values = dict[i64, i64]().unwrap()
for n in range(100).unwrap():
    values.insert(n, n * 2).unwrap()
mut valid = True
for n in range(0, 100, 2).unwrap():
    match values.pop(n):
        case Some(value):
            valid = valid and value == n * 2
        case Nothing:
            valid = False
for n in range(1, 100, 2).unwrap():
    match values.get(n):
        case Some(value):
            valid = valid and value == n * 2
        case Nothing:
            valid = False
print(valid).unwrap()
print(values.keys().unwrap() == [n for n in range(1, 100, 2).unwrap()].unwrap()).unwrap()
for n in range(0, 100, 2).unwrap():
    values.insert(n, n).unwrap()
print(len(values)).unwrap()
print(values.pop(0)).unwrap()
print(values.pop(99)).unwrap()
"#,
        "True\nTrue\n100\nOption[i64].Some(0)\nOption[i64].Some(198)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn removal_and_reinsertion_reuse_capacity_with_allocation_disabled() {
    native(
        r#"
mut values = {"a": [1].unwrap(), "b": [2].unwrap(), "c": [3].unwrap()}.unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
missing = values.pop("absent")
removed = values.pop("b")
match removed:
    case Some(items):
        values.insert("d", items).unwrap()
    case Nothing:
        print("unexpected").unwrap()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(missing).unwrap()
print(values).unwrap()
"#,
        "Option[list[i64]].Nothing\n{\"a\": [1], \"c\": [3], \"d\": [2]}",
    );
}

#[rstest]
#[case("data = {1: 2}.unwrap()\nprint(data.pop(1)).unwrap()", "mutable")]
#[case(
    "mut data = {1: 2}.unwrap()\nprint(data.pop()).unwrap()",
    "pop requires one key argument"
)]
#[case(
    "mut data = {1: 2}.unwrap()\nprint(data.pop(1, 0)).unwrap()",
    "pop requires one key argument"
)]
#[case(
    "mut data = {1u8: 2}.unwrap()\nprint(data.pop(1)).unwrap()",
    "expected u8"
)]
#[case(
    "print({1: 2}.unwrap().pop(1)).unwrap()",
    "pop requires a mutable list or dictionary"
)]
#[case(
    "mut data = {1}.unwrap()\nprint(data.pop(0)).unwrap()",
    "pop requires a mutable list or dictionary"
)]
#[case(
    "mut data = {1: [2].unwrap()}.unwrap()\nloan = &data\nremoved = data.pop(1)\nprint(loan).unwrap()",
    "borrow"
)]
#[case(
    "mut data = {1: [2].unwrap()}.unwrap()\nremoved = data.pop(1)\ndrop(removed)\nprint(removed).unwrap()",
    "moved"
)]
fn rejects_invalid_removal(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

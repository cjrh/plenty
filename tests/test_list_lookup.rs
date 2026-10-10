//! Optional list reads preserve owners and never allocate.
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
fn lookup_handles_signed_bounds_and_preserves_the_list() {
    native(r#"
items = [0, 10, 20].unwrap()
for index in [-4, -3, -1, 0, 2, 3, -9223372036854775808, 9223372036854775807].unwrap():
    print(items.get(index)).unwrap()
print(list[i64]().unwrap().get(0)).unwrap()
print(items).unwrap()
print([False].unwrap().get(0)).unwrap()
print([1.5f32].unwrap().get(0)).unwrap()
"#, "Option[i64].Nothing\nOption[i64].Some(0)\nOption[i64].Some(20)\nOption[i64].Some(0)\nOption[i64].Some(20)\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\n[0, 10, 20]\nOption[bool].Some(False)\nOption[f32].Some(1.5)");
}

#[test]
fn managed_results_survive_replacement_and_list_destruction() {
    native(r#"
enum Value:
    Text(str)
mut words = [("é" + "\0🙂").unwrap()].unwrap()
found = words.get(0)
words[0] = "replacement"
drop(words)
print(found).unwrap()
values = [Value.Text(("A" + "da").unwrap())].unwrap()
value = values.get(-1)
drop(values)
print(value).unwrap()
print([("B" + "ea").unwrap()].unwrap().get(0)).unwrap()
options: list[Option[str]] = [Nothing, Some("hello")].unwrap()
print(options.get(0)).unwrap()
print(options.get(1)).unwrap()
print(options.get(2)).unwrap()
"#, "Option[str].Some(\"é\\0🙂\")\nOption[Value].Some(Value.Text(\"Ada\"))\nOption[str].Some(\"Bea\")\nOption[Option[str]].Some(Option[str].Nothing)\nOption[Option[str]].Some(Option[str].Some(\"hello\"))\nOption[Option[str]].Nothing");
}

#[test]
fn borrowed_fields_indices_and_operand_order_work() {
    native(
        r#"
class Storage:
    items: list[i64]
def lookup(items: &list[i64], index: &i64) -> Option[i64]:
    items.get(index)
def items() -> list[i64]:
    print("items").unwrap()
    [42].unwrap()
def index() -> i64:
    print("index").unwrap()
    0
storage = Storage([10, 20].unwrap())
position = -1
print(lookup(&storage.items, &position)).unwrap()
print(items().get(index())).unwrap()
print(storage.items).unwrap()
"#,
        "Option[i64].Some(20)\nitems\nindex\nOption[i64].Some(42)\n[10, 20]",
    );
}

#[test]
fn lookup_propagates_absence_without_consuming_the_source() {
    native(
        r#"
def sum_two(items: &list[i64]) -> Option[i64]:
    Some(items.get(0)? + items.get(1)?)
items = [20, 22].unwrap()
short = [20].unwrap()
print(sum_two(&items)).unwrap()
print(sum_two(&short)).unwrap()
print(short).unwrap()
"#,
        "Option[i64].Some(42)\nOption[i64].Nothing\n[20]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn hits_misses_and_retention_work_with_allocation_disabled() {
    native(r#"
enum Value:
    Text(str)
items = [("A" + "da").unwrap()].unwrap()
values = [Value.Text(("B" + "ea").unwrap())].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
found = items.get(0)
missing = items.get(-2)
value = values.get(0)
again = found
drop(items)
drop(values)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(found).unwrap()
print(again).unwrap()
print(missing).unwrap()
print(value).unwrap()
"#, "Option[str].Some(\"Ada\")\nOption[str].Some(\"Ada\")\nOption[str].Nothing\nOption[Value].Some(Value.Text(\"Bea\"))");
}

#[rstest]
#[case("[1].unwrap().get()", "get requires one index argument")]
#[case("[1].unwrap().get(0, 1)", "get requires one index argument")]
#[case("[1].unwrap().get(0u8)", "expected i64")]
#[case("[1].unwrap().get(0.0)", "expected i64")]
#[case(
    "[[1].unwrap()].unwrap().get(0)",
    "get cannot return owned list values"
)]
#[case(
    "class Item:\n    value: i64\n[Item(1)].unwrap().get(0)",
    "get cannot return owned list values"
)]
#[case(
    "mut items = [1].unwrap()\nloan = &mut items\nitems.get(0)\nloan.append(2).unwrap()",
    "borrow"
)]
fn rejects_invalid_lookup(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

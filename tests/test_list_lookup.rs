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
items = [0, 10, 20]
for index in [-4, -3, -1, 0, 2, 3, -9223372036854775808, 9223372036854775807]:
    print(items.get(index))
print(list[i64]().get(0))
print(items)
print([False].get(0))
print([1.5f32].get(0))
"#, "Option[i64].Nothing\nOption[i64].Some(0)\nOption[i64].Some(20)\nOption[i64].Some(0)\nOption[i64].Some(20)\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\n[0, 10, 20]\nOption[bool].Some(False)\nOption[f32].Some(1.5)");
}

#[test]
fn managed_results_survive_replacement_and_list_destruction() {
    native(r#"
enum Value:
    Text(str)
mut words = ["é" + "\0🙂"]
found = words.get(0)
words[0] = "replacement"
drop(words)
print(found)
values = [Value.Text("A" + "da")]
value = values.get(-1)
drop(values)
print(value)
print(["B" + "ea"].get(0))
options: list[Option[str]] = [Nothing, Some("hello")]
print(options.get(0))
print(options.get(1))
print(options.get(2))
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
    print("items")
    [42]
def index() -> i64:
    print("index")
    0
storage = Storage([10, 20])
position = -1
print(lookup(&storage.items, &position))
print(items().get(index()))
print(storage.items)
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
items = [20, 22]
short = [20]
print(sum_two(&items))
print(sum_two(&short))
print(short)
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
items = ["A" + "da"]
values = [Value.Text("B" + "ea")]
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
found = items.get(0)
missing = items.get(-2)
value = values.get(0)
again = found
drop(items)
drop(values)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(found)
print(again)
print(missing)
print(value)
"#, "Option[str].Some(\"Ada\")\nOption[str].Some(\"Ada\")\nOption[str].Nothing\nOption[Value].Some(Value.Text(\"Bea\"))");
}

#[rstest]
#[case("[1].get()", "get requires one index argument")]
#[case("[1].get(0, 1)", "get requires one index argument")]
#[case("[1].get(0u8)", "expected i64")]
#[case("[1].get(0.0)", "expected i64")]
#[case("[[1]].get(0)", "get cannot return owned list values")]
#[case(
    "class Item:\n    value: i64\n[Item(1)].get(0)",
    "get cannot return owned list values"
)]
#[case(
    "mut items = [1]\nloan = &mut items\nitems.get(0)\nloan.append(2)",
    "borrow"
)]
fn rejects_invalid_lookup(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

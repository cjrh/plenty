//! Optional dictionary lookup without implicit copies or runtime allocation.
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
fn lookup_distinguishes_missing_keys_from_zero_false_and_empty_values() {
    native(r#"
numbers = {"zero": 0, "negative": -2}
print(numbers.get("zero"))
print(numbers.get("negative"))
print(numbers.get("missing"))
print(dict[str, i64]().get("missing"))
flags = {False: False, True: True}
print(flags.get(False))
text = {0u8: "", 1u8: "é\0🙂"}
print(text.get(0u8))
print(text.get(1u8))
print(text.get(2u8))
print({1: 2.5f32}.get(1))
print({1: 3.5}.get(1))
print(numbers)
"#, "Option[i64].Some(0)\nOption[i64].Some(-2)\nOption[i64].Nothing\nOption[i64].Nothing\nOption[bool].Some(False)\nOption[str].Some(\"\")\nOption[str].Some(\"é\\0🙂\")\nOption[str].Nothing\nOption[f32].Some(2.5)\nOption[f64].Some(3.5)\n{\"zero\": 0, \"negative\": -2}");
}

#[test]
fn nested_options_results_and_immutable_enums_preserve_tags() {
    native(r#"
enum Value:
    Empty
    Text(str)
options: dict[i64, Option[str]] = {1: Nothing, 2: Some("hello")}
results: dict[i64, Result[(), i64]] = {1: Ok(()), 2: Err(4)}
values = {1: Value.Empty, 2: Value.Text("hello")}
print(options.get(1))
print(options.get(2))
print(options.get(3))
print(results.get(1))
print(results.get(2))
print(values.get(1))
print(values.get(2))
"#, "Option[Option[str]].Some(Option[str].Nothing)\nOption[Option[str]].Some(Option[str].Some(\"hello\"))\nOption[Option[str]].Nothing\nOption[Result[(), i64]].Some(Result[(), i64].Ok(()))\nOption[Result[(), i64]].Some(Result[(), i64].Err(4))\nOption[Value].Some(Value.Empty)\nOption[Value].Some(Value.Text(\"hello\"))");
}

#[test]
fn lookup_borrows_receiver_and_key_and_survives_replacement_and_drop() {
    native(
        r#"
class Directory:
    names: dict[str, str]
def lookup(names: &dict[str, str], key: &str) -> Option[str]:
    names.get(key)
mut directory = Directory({"name": "A" + "da"})
key = "na" + "me"
found = lookup(&directory.names, &key)
directory.names[key] = "Bea"
print(directory.names.get(key))
print(key)
drop(directory)
print(found)
print({"name": "C" + "am"}.get(key))
"#,
        "Option[str].Some(\"Bea\")\nname\nOption[str].Some(\"Ada\")\nOption[str].Some(\"Cam\")",
    );
}

#[test]
fn lookup_preserves_the_deepest_supported_inline_tag_path() {
    let mut source = String::from("type T0 = i64\n");
    for depth in 1..=63 {
        source.push_str(&format!("type T{depth} = Option[T{}]\n", depth - 1));
    }
    let value = format!("{}42{}", "Some(".repeat(63), ")".repeat(63));
    source.push_str(&format!(
        "value: T63 = {value}\ndata = {{1: value}}\nfound = data.get(1)\nmatch found:\n    case Some(inner):\n        print(inner == value)\n    case Nothing:\n        print(False)\nmatch data.get(2):\n    case Some(inner):\n        print(False)\n    case Nothing:\n        print(True)\n"
    ));
    native(&source, "True\nTrue");
}

#[test]
fn operands_evaluate_once_in_order_and_class_methods_remain_available() {
    native(
        r#"
def data() -> dict[i64, i64]:
    print("receiver")
    {1: 42}
def key() -> i64:
    print("key")
    1
class Custom:
    def get(self, key: i64) -> i64:
        key
print(data().get(key()))
print(Custom().get(7))
"#,
        "receiver\nkey\nOption[i64].Some(42)\n7",
    );
}

#[test]
fn optional_lookup_composes_with_propagation() {
    native(
        r#"
def add(data: &dict[str, i64]) -> Option[i64]:
    first = data.get("first")?
    second = data.get("second")?
    Some(first + second)
complete = {"first": 20, "second": 22}
missing = {"first": 20}
print(add(&complete))
print(add(&missing))
print(missing)
"#,
        "Option[i64].Some(42)\nOption[i64].Nothing\n{\"first\": 20}",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn lookup_and_result_retention_allocate_nothing() {
    native(r#"
enum Value:
    Text(str)
strings = {"na" + "me": "A" + "da"}
values = {1: Value.Text("B" + "ea")}
key = "n" + "ame"
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
found = strings.get(key)
missing = strings.get("missing")
value = values.get(1)
again = found
drop(strings)
drop(values)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(found)
print(again)
print(missing)
print(value)
"#, "Option[str].Some(\"Ada\")\nOption[str].Some(\"Ada\")\nOption[str].Nothing\nOption[Value].Some(Value.Text(\"Bea\"))");
}

#[test]
fn lookup_uses_the_hash_index_after_growth_and_replacement() {
    native(
        r#"
mut values = dict[i64, i64]()
for n in range(100):
    values[n] = n * 2
values[50] = 123
mut valid = True
for n in range(100):
    match values.get(n):
        case Some(value):
            valid = valid and value == (123 if n == 50 else n * 2)
        case Nothing:
            valid = False
print(valid)
print(values.get(100))
print(len(values))
"#,
        "True\nOption[i64].Nothing\n100",
    );
}

#[rstest]
#[case("print({1: 2}.get())", "get requires one key argument")]
#[case("print({1: 2}.get(1, 0))", "get requires one key argument")]
#[case("print({1u8: 2}.get(1))", "expected u8")]
#[case("print({1: 2}.get(\"key\"))", "expected i64")]
#[case("print([1].get(0))", "unsupported method `get`")]
#[case("print({1: [2]}.get(1))", "get cannot return owned dictionary values")]
#[case(
    "class Item:\n    value: i64\nprint({1: Item(2)}.get(1))",
    "get cannot return owned dictionary values"
)]
#[case(
    "enum Item:\n    Values(list[i64])\nprint({1: Item.Values([2])}.get(1))",
    "get cannot return owned dictionary values"
)]
#[case(
    "mut data = {1: 2}\nloan = &mut data\nprint(data.get(1))\nloan[1] = 3",
    "borrow"
)]
fn rejects_invalid_lookup(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

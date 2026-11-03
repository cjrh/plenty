//! Optional dictionary lookup without implicit copies or runtime allocation.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn lookup_distinguishes_missing_keys_from_zero_false_and_empty_values() {
    native(
        r#"
numbers = {"zero": 0, "negative": -2}.unwrap()
print(numbers.get("zero")).unwrap()
print(numbers.get("negative")).unwrap()
print(numbers.get("missing")).unwrap()
print(dict[str, i64]().unwrap().get("missing")).unwrap()
flags = {False: False, True: True}.unwrap()
print(flags.get(False)).unwrap()
text = {0u8: "", 1u8: "é\0🙂"}.unwrap()
print(text.get(0u8)).unwrap()
print(text.get(1u8)).unwrap()
print(text.get(2u8)).unwrap()
print({1: 2.5f32}.unwrap().get(1)).unwrap()
print({1: 3.5}.unwrap().get(1)).unwrap()
print(numbers).unwrap()
"#,
        "Option[i64].Some(0)\nOption[i64].Some(-2)\nOption[i64].Nothing\nOption[i64].Nothing\nOption[bool].Some(False)\nOption[str].Some(\"\")\nOption[str].Some(\"é\\0🙂\")\nOption[str].Nothing\nOption[f32].Some(2.5)\nOption[f64].Some(3.5)\n{\"zero\": 0, \"negative\": -2}",
    );
}

#[test]
fn nested_options_results_and_immutable_enums_preserve_tags() {
    native(
        r#"
enum Value:
    Empty
    Text(str)
options: dict[i64, Option[str]] = {1: Nothing, 2: Some("hello")}.unwrap()
results: dict[i64, Result[(), i64]] = {1: Ok(()), 2: Err(4)}.unwrap()
values = {1: Value.Empty, 2: Value.Text("hello")}.unwrap()
print(options.get(1)).unwrap()
print(options.get(2)).unwrap()
print(options.get(3)).unwrap()
print(results.get(1)).unwrap()
print(results.get(2)).unwrap()
print(values.get(1)).unwrap()
print(values.get(2)).unwrap()
"#,
        "Option[Option[str]].Some(Option[str].Nothing)\nOption[Option[str]].Some(Option[str].Some(\"hello\"))\nOption[Option[str]].Nothing\nOption[Result[(), i64]].Some(Result[(), i64].Ok(()))\nOption[Result[(), i64]].Some(Result[(), i64].Err(4))\nOption[Value].Some(Value.Empty)\nOption[Value].Some(Value.Text(\"hello\"))",
    );
}

#[test]
fn lookup_borrows_receiver_and_key_and_survives_replacement_and_drop() {
    native(
        r#"
class Directory:
    names: dict[str, str]
def lookup(names: &dict[str, str], key: &str) -> Option[str]:
    names.get(key)
mut directory = Directory({"name": ("A" + "da").unwrap()}.unwrap())
key = ("na" + "me").unwrap()
found = lookup(&directory.names, &key)
directory.names.insert(key, "Bea").unwrap()
print(directory.names.get(key)).unwrap()
print(key).unwrap()
drop(directory)
print(found).unwrap()
print({"name": ("C" + "am").unwrap()}.unwrap().get(key)).unwrap()
"#,
        "Option[str].Some(\"Bea\")\nname\nOption[str].Some(\"Ada\")\nOption[str].Some(\"Cam\")",
    );
}

#[test]
fn lookup_preserves_the_deepest_supported_inline_tag_path() {
    let mut source = String::from("type T0 = i64\n");
    for depth in 1..=62 {
        source.push_str(&format!("type T{depth} = Option[T{}]\n", depth - 1));
    }
    let value = format!("{}42{}", "Some(".repeat(62), ")".repeat(62));
    source.push_str(&format!(
        "value: T62 = {value}\ndata = {{1: value}}.unwrap()\nfound = data.get(1)\nmatch found:\n    case Some(inner):\n        print(inner == value).unwrap()\n    case Nothing:\n        print(False).unwrap()\nmatch data.get(2):\n    case Some(inner):\n        print(False).unwrap()\n    case Nothing:\n        print(True).unwrap()\n"
    ));
    native(&source, "True\nTrue");
}

#[test]
fn operands_evaluate_once_in_order_and_class_methods_remain_available() {
    native(
        r#"
def data() -> dict[i64, i64]:
    print("receiver").unwrap()
    {1: 42}.unwrap()
def key() -> i64:
    print("key").unwrap()
    1
class Custom:
    def get(self, key: i64) -> i64:
        key
print(data().get(key())).unwrap()
print(Custom().get(7)).unwrap()
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
complete = {"first": 20, "second": 22}.unwrap()
missing = {"first": 20}.unwrap()
print(add(&complete)).unwrap()
print(add(&missing)).unwrap()
print(missing).unwrap()
"#,
        "Option[i64].Some(42)\nOption[i64].Nothing\n{\"first\": 20}",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn lookup_and_result_retention_allocate_nothing() {
    native(
        r#"
enum Value:
    Text(str)
strings = {("na" + "me").unwrap(): ("A" + "da").unwrap()}.unwrap()
values = {1: Value.Text(("B" + "ea").unwrap())}.unwrap()
key = ("n" + "ame").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
found = strings.get(key)
missing = strings.get("missing")
value = values.get(1)
again = found
drop(strings)
drop(values)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(found).unwrap()
print(again).unwrap()
print(missing).unwrap()
print(value).unwrap()
"#,
        "Option[str].Some(\"Ada\")\nOption[str].Some(\"Ada\")\nOption[str].Nothing\nOption[Value].Some(Value.Text(\"Bea\"))",
    );
}

#[test]
fn lookup_uses_the_hash_index_after_growth_and_replacement() {
    native(
        r#"
mut values = dict[i64, i64]().unwrap()
for n in range(100):
    values.insert(n, n * 2).unwrap()
values.insert(50, 123).unwrap()
mut valid = True
for n in range(100):
    match values.get(n):
        case Some(value):
            valid = valid and value == (123 if n == 50 else n * 2)
        case Nothing:
            valid = False
print(valid).unwrap()
print(values.get(100)).unwrap()
print(len(values)).unwrap()
"#,
        "True\nOption[i64].Nothing\n100",
    );
}

#[rstest]
#[case(
    "print({1: 2}.unwrap().get()).unwrap()",
    "get requires one key argument"
)]
#[case(
    "print({1: 2}.unwrap().get(1, 0, 3)).unwrap()",
    "get requires one key argument"
)]
#[case("key = 1\nprint({1u8: 2}.unwrap().get(key)).unwrap()", "expected u8")]
#[case("print({1: 2}.unwrap().get(\"key\")).unwrap()", "expected i64")]
#[case("print({1}.unwrap().get(0)).unwrap()", "unsupported method `get`")]
#[case(
    "print({1: [2].unwrap()}.unwrap().get(1)).unwrap()",
    "get cannot return owned dictionary values"
)]
#[case(
    "class Item:\n    value: i64\nprint({1: Item(2)}.unwrap().get(1)).unwrap()",
    "get cannot return owned dictionary values"
)]
#[case(
    "enum Item:\n    Values(list[i64])\nprint({1: Item.Values([2].unwrap())}.unwrap().get(1)).unwrap()",
    "get cannot return owned dictionary values"
)]
#[case(
    "mut data = {1: 2}.unwrap()\nloan = &mut data\nprint(data.get(1)).unwrap()\nloan.insert(1, 3).unwrap()",
    "borrow"
)]
fn rejects_invalid_lookup(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn structural_class_keys_lookup_views_snapshots_and_transfer() {
    support::assert_stdout(
        r#"
class Key:
    text: str
    n: i64
mut data = {Key("a", 1): "first", Key("b", 2): "second"}.unwrap()
probe = Key("a", 1)
print(data[&probe], data.get(&probe).unwrap(), probe.text).unwrap()
match data.get_mut(&probe):
    case Some(value):
        *value = "changed"
    case Nothing:
        pass
borrowed = &data[&probe]
print(*borrowed).unwrap()
keys = data.keys().to_set().unwrap()
combined = keys.union(data.keys()).unwrap()
snapshot = copy(data).unwrap().keys_snapshot().unwrap()
for key, value in data.items():
    print(key.text, *value).unwrap()
print(data.pop(&probe).unwrap(), probe.text).unwrap()
data.update({Key("b", 2): "updated", Key("c", 3): "third"}.unwrap()).unwrap()
owned = data.into_keys().unwrap()
print(len(keys), len(combined), len(snapshot), len(owned)).unwrap()
for key in owned:
    print(key.text).unwrap()
print(Key("a", 1) in keys, snapshot[0].text).unwrap()
"#,
        "first first a\nchanged\na changed\nb second\nchanged a\n2 2 2 2\nb\nc\nTrue a\n",
    );
}

#[test]
fn structural_class_keys_cannot_be_implicitly_duplicated_or_mutated() {
    for (body, diagnostic) in [
        (
            "key = Key(1)\nkeys = {key}.unwrap()\nprint(key.n).unwrap()",
            "moved",
        ),
        (
            "data = {Key(1): 2}.unwrap()\nkeys = data.keys_snapshot().unwrap()",
            "owned temporary",
        ),
        (
            "mut keys = {Key(1)}.unwrap()\nfor key in &keys:\n    key.n = 2",
            "shared",
        ),
    ] {
        let source = format!("class Key:\n    n: i64\n{body}\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn structural_class_keys_failed_extraction_releases_owned_source() {
    for budget in 0..=2 {
        support::assert_visible_stdout(
            &format!(
                r#"
class Key:
    text: str
    n: i64
data = {{Key(("hello" + "").unwrap(), 1): ("value" + "").unwrap()}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = data.into_keys()
print("__test_restore_allocations__").unwrap()
print(result.is_ok()).unwrap()
match result:
    case Ok(keys):
        print(keys[0].text).unwrap()
    case Err(error):
        pass
"#
            ),
            if budget < 2 { "False" } else { "True\nhello" },
        );
    }
}

#[test]
fn structural_class_keys_generic_bounds_comprehensions_and_values() {
    support::assert_stdout(
        r#"
class Key[T: KeyType]:
    value: T
    label: str
def distinct[K: KeyType](items: list[K]) -> Result[set[K], AllocError]:
    {item for item in items}
keys = distinct([Key(1, "a"), Key(1, "a"), Key(2, "b")].unwrap()).unwrap()
print(len(keys)).unwrap()
data = {Key(n, "x"): n for n in range(1, 3)}.unwrap()
print(data[Key(1, "x")], data[Key(2, "x")]).unwrap()
values = data.into_values().unwrap()
print(values).unwrap()
"#,
        "2\n1 2\n[1, 2]\n",
    );
}

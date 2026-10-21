//! Allocation-free defaults and directly lowered iteration adapters.
mod support;
use rstest::rstest;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end(), "{source}");
}

#[test]
fn defaults_preserve_present_zero_false_empty_and_nested_values() {
    native(
        r#"
print(Some(0).unwrap_or(9)).unwrap()
print(Option[i64].Nothing.unwrap_or(9)).unwrap()
print(Some(False).unwrap_or(True)).unwrap()
print(Some("").unwrap_or("missing")).unwrap()
print(Some(Option[i64].Nothing).unwrap_or(Some(9))).unwrap()
numbers = {"zero": 0}.unwrap()
print(numbers.get("zero", 9)).unwrap()
print(numbers.get("missing", 9)).unwrap()
print(numbers.get("missing")).unwrap()
flags = {1: False}.unwrap()
print(flags.get(1, True)).unwrap()
print({1: ""}.unwrap().get(1, "missing")).unwrap()
small: u8 = Nothing.unwrap_or(7)
print(small).unwrap()
print(Some(1u8).unwrap_or(0)).unwrap()
Some(()).unwrap_or(())
"#,
        "0\n9\nFalse\n\nOption[i64].Nothing\n0\n9\nOption[i64].Nothing\nFalse\n\n7\n1",
    );
}

#[test]
fn arguments_are_eager_and_evaluated_once_in_source_order() {
    native(
        r#"
def source() -> Option[i64]:
    print("option").unwrap()
    Some(4)
def data() -> dict[str, i64]:
    print("dictionary").unwrap()
    {"key": 5}.unwrap()
def key() -> str:
    print("key").unwrap()
    "key"
def fallback() -> i64:
    print("default").unwrap()
    9
print(source().unwrap_or(fallback())).unwrap()
print(data().get(key(), fallback())).unwrap()
"#,
        "option\ndefault\n4\ndictionary\nkey\ndefault\n5",
    );
}

#[test]
fn ok_or_preserves_typed_errors_and_composes_with_propagation() {
    native(
        r#"
enum Missing:
    Key(str)
def find(data: &dict[str, i64]) -> Result[i64, Missing]:
    value = data.get("answer").ok_or(Missing.Key("answer"))?
    Ok(value + 1)
print(str.repr(find(&{"answer": 41}.unwrap())).unwrap()).unwrap()
print(str.repr(find(&dict[str, i64]().unwrap())).unwrap()).unwrap()
result: Result[u8, str] = Nothing.ok_or("absent")
print(str.repr(result).unwrap()).unwrap()
print(str.repr(Some(()).ok_or(())).unwrap()).unwrap()
"#,
        "Result[i64, Missing].Ok(42)\nResult[i64, Missing].Err(Missing.Key(\"answer\"))\nResult[u8, str].Err(\"absent\")\nResult[(), ()].Ok(())",
    );
}

#[test]
fn owned_defaults_and_errors_drop_unused_values_exactly_once() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
present = Some(Guard("payload")).unwrap_or(Guard("unused"))
print("selected").unwrap()
drop(present)
absent = Option[Guard].Nothing.unwrap_or(Guard("fallback"))
drop(absent)
result = Some(Guard("success")).ok_or(Guard("unused error"))
drop(result)
failed = Option[Guard].Nothing.ok_or(Guard("error"))
drop(failed)
"#,
        "unused\nselected\npayload\nfallback\nunused error\nsuccess\nerror",
    );
}

#[test]
fn enumeration_and_zip_work_in_loops_and_comprehensions() {
    native(
        r#"
for index, value in enumerate("é🙂", -2):
    print((index, value)).unwrap()
for pair in zip(range(3), "ab"):
    print(pair).unwrap()
print([index + value for index, value in enumerate(range(3), 10)].unwrap()).unwrap()
print([a + b for a, b in zip(range(4), range(10, 12))].unwrap()).unwrap()
for index, pair in enumerate(zip(range(2), "ab")):
    print((index, pair)).unwrap()
for pair in zip(enumerate("ab"), range(2)):
    print(pair).unwrap()
"#,
        "(-2, \"é\")\n(-1, \"🙂\")\n(0, \"a\")\n(1, \"b\")\n[10, 12, 14]\n[10, 12]\n(0, (0, \"a\"))\n(1, (1, \"b\"))\n((0, \"a\"), 0)\n((1, \"b\"), 1)",
    );
}

#[test]
fn adapters_support_borrowed_and_mutable_elements() {
    native(
        r#"
class Item:
    value: i64
mut items = [Item(2), Item(4)].unwrap()
for index, item in enumerate(&mut items, 10):
    item.value += index
for item, index in zip(&items, range(2)):
    print(item.value + index).unwrap()
print([index + item.value for index, item in enumerate(&items)].unwrap()).unwrap()
print(len(items)).unwrap()
mut other = [1, 2].unwrap()
for left, right in zip(&mut other, &items):
    *left += right.value
print(other).unwrap()
"#,
        "12\n16\n[12, 16]\n2\n[13, 17]",
    );
}

#[test]
fn empty_inputs_continue_break_and_maximum_final_index() {
    native(
        r#"
for index, value in enumerate(range(0), 9223372036854775807):
    print("unreachable").unwrap()
for index, value in enumerate(range(1), 9223372036854775807):
    print(index).unwrap()
for a, b in zip(range(0), range(2)):
    print("unreachable").unwrap()
for a, b in zip(range(2), range(0)):
    print("unreachable").unwrap()
for index, value in enumerate(range(5)):
    if index == 1:
        continue
    if index == 3:
        break
    print((index, value)).unwrap()
for a, b in zip(range(5), range(10, 15)):
    if a == 1:
        continue
    if a == 3:
        break
    print((a, b)).unwrap()
"#,
        "9223372036854775807\n(0, 0)\n(2, 2)\n(0, 10)\n(2, 12)",
    );
}

#[test]
fn zip_short_circuits_generator_advancement_and_cleans_up_owned_items() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def left() -> Generator[Guard]:
    print("left 1").unwrap()
    yield Guard("first")
    print("left 2").unwrap()
    yield Guard("extra")
def right() -> Generator[i64]:
    print("right 1").unwrap()
    yield 1
for guard, number in zip(left(), right()):
    print(number).unwrap()
    drop(guard)
for guard, number in zip([Guard("remaining")].unwrap(), range(0)):
    print("unreachable").unwrap()
"#,
        "left 1\nright 1\n1\nfirst\nleft 2\nextra\nremaining",
    );
}

#[test]
fn custom_methods_and_functions_keep_their_names() {
    native(
        r#"
class Custom:
    def unwrap_or(self, value: i64, extra: i64) -> i64:
        value + extra
    def ok_or(self, value: i64) -> i64:
        value + 1
    def get(self, key: i64, default: i64) -> i64:
        key + default
def enumerate(value: i64) -> range[i64]:
    range(value)
def zip(value: i64) -> range[i64]:
    range(value)
print(Custom().unwrap_or(1, 2)).unwrap()
print(Custom().ok_or(3)).unwrap()
print(Custom().get(3, 4)).unwrap()
boxed = Box(Custom()).unwrap()
print(boxed.unwrap_or(1, 2)).unwrap()
print(boxed.ok_or(3)).unwrap()
print(Box(Custom()).unwrap().ok_or(3)).unwrap()
for value in enumerate(1):
    print(value).unwrap()
for value in zip(1):
    print(value).unwrap()
"#,
        "3\n4\n7\n3\n4\n4\n0\n0",
    );
}

#[test]
fn propagation_in_defaults_and_adapter_inputs_drops_pending_owners() {
    native(
        r#"
class Guard:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def missing() -> Result[Guard, str]:
    Err("missing")
def choose() -> Result[Guard, str]:
    Ok(Some(Guard("receiver")).unwrap_or(missing()?))
def failed_input() -> Result[list[i64], str]:
    Err("input")
def paired() -> Result[(), str]:
    for item, value in zip([Guard("left source")].unwrap(), failed_input()? ):
        pass
    Ok(())
print(str.repr(choose()).unwrap()).unwrap()
print(str.repr(paired()).unwrap()).unwrap()
"#,
        "receiver\nResult[Guard, str].Err(\"missing\")\nleft source\nResult[(), str].Err(\"input\")",
    );
}

#[test]
fn enumeration_index_overflow_reports_its_source_location() {
    let output = support::run(
        "for index, value in enumerate(range(2), 9223372036854775807):\n    print(index).unwrap()",
    );
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("overflow"), "{error}");
    assert!(error.contains("3:"), "{error}");
}

#[test]
fn input_evaluation_order_and_callable_bindings_are_preserved() {
    native(
        r#"
def source(label: str) -> range[i64]:
    print(label).unwrap()
    range(1)
def start() -> i64:
    print("start").unwrap()
    5
for index, value in enumerate(source("source"), start()):
    print(index).unwrap()
for left, right in zip(source("left"), source("right")):
    print(left + right).unwrap()
enumerate = source
for value in enumerate("callable"):
    print(value).unwrap()
"#,
        "source\nstart\n5\nleft\nright\n0\ncallable\n0",
    );
}

#[test]
fn hash_collections_and_filtered_comprehensions_keep_iteration_semantics() {
    native(
        r#"
data = {"a": 10, "b": 20}.unwrap()
for index, key in enumerate(&data):
    print((index, *key)).unwrap()
for key, value in zip(&data, range(2)):
    print((*key, value)).unwrap()
print([index for index, value in enumerate(range(4)) if value % 2 == 0].unwrap()).unwrap()
print([a + b for a, b in zip(range(4), range(4)) if a % 2 == 0].unwrap()).unwrap()
"#,
        "(0, \"a\")\n(1, \"b\")\n(\"a\", 0)\n(\"b\", 1)\n[0, 2]\n[0, 4]",
    );
}

#[test]
fn adapters_resolve_in_imported_generic_functions_and_generators() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("helpers.plenty"),
        r#"
pub def counted[T](values: &list[T]) -> i64:
    mut total = 0
    for index, value in enumerate(values):
        total += index
    total
pub def paired() -> Generator[i64]:
    for a, b in zip(range(2), range(10, 12)):
        yield a + b
"#,
    )
    .unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(
        &source,
        r#"
import helpers
def main() -> ():
    values = ["a", "b", "c"].unwrap()
    print(helpers.counted(&values)).unwrap()
    for index, value in enumerate(helpers.paired()):
        print((index, value)).unwrap()
    for a, b in zip(helpers.paired(), zip(helpers.paired(), helpers.paired())):
        print((a, b)).unwrap()
"#,
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &executable, Some(dir.path())).unwrap();
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\n(0, 10)\n(1, 12)\n(10, (10, 10))\n(12, (12, 12))\n"
    );
}

#[rstest]
#[case("Some(1).unwrap_or()", "unwrap_or requires one argument")]
#[case("Some(1).ok_or(1, 2)", "ok_or requires one argument")]
#[case("(1).unwrap_or(2)", "unwrap_or requires an Option")]
#[case("Result[i64, str].Ok(1).ok_or(2)", "ok_or requires an Option")]
#[case("Some(1).unwrap_or(False)", "expected")]
#[case("{1: 2}.unwrap().get(1, False)", "expected")]
#[case("[1].unwrap().get(0, 2)", "get requires one index")]
#[case(
    "{1: [2].unwrap()}.unwrap().get(1, [3].unwrap())",
    "get cannot return owned dictionary values"
)]
#[case("for a, b in enumerate():\n    pass", "enumerate requires an iterable")]
#[case("for a, b in zip(range(2)):\n    pass", "zip requires two iterables")]
#[case("items = enumerate(range(2))", "supported directly in for loops")]
#[case(
    "items = [1].unwrap()\nfor pair in enumerate(&items):\n    pass",
    "requires two unpacked"
)]
#[case(
    "mut items = [1].unwrap()\nfor index, value in enumerate(&items):\n    items.append(2).unwrap()\n    print(value).unwrap()",
    "borrow"
)]
#[case(
    "mut items = [1].unwrap()\nfor a, b in zip(&mut items, &mut items):\n    *a = *b",
    "borrow"
)]
#[case(
    "class Item:\n    value: i64\noption = Some(Item(1))\nvalue = option.unwrap_or(Item(2))\nprint(option).unwrap()",
    "moved"
)]
fn rejects_invalid_conveniences(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn helpers_allocate_nothing_after_inputs_exist() {
    native(
        r#"
data = {"key": ("long " + "payload").unwrap()}.unwrap()
option = Some(("long " + "value").unwrap())
fallback = ("long " + "default").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
found = data.get("key", fallback)
missing = data.get("missing", fallback)
value = option.unwrap_or(fallback)
result = Option[str].Nothing.ok_or(fallback)
mut total = 0
for index, number in enumerate(range(3)):
    total += index + number
for a, b in zip(range(2), range(10, 12)):
    total += a + b
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
drop(data)
print(found).unwrap()
print(missing).unwrap()
print(value).unwrap()
print(str.repr(result).unwrap()).unwrap()
print(total).unwrap()
"#,
        "long payload\nlong default\nlong value\nResult[str, str].Err(\"long default\")\n28",
    );
}

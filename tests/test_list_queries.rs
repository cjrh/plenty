//! Observed list queries with allocation-free equality and optional positions.
mod support;
use rstest::rstest;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
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
fn queries_find_first_last_missing_and_content_equal_elements() {
    native(r#"
def locate(items: &list[str], query: &str) -> Option[i64]:
    items.find(query)
items = ["é" + "", "other", "é"]
query = "" + "é"
print(locate(&items, &query))
print(items.rfind(query))
print(items.count(query))
print(items.find("missing"))
print(list[i64]().find(0))
print(list[i64]().rfind(0))
print(list[i64]().count(0))
print([False, True, False].count(False))
print([1u8, 2u8, 1u8].rfind(1u8))
print([-1, 2, -1].find(-1))
print(len(items))
"#, "Option[i64].Some(0)\nOption[i64].Some(2)\n2\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\n0\n2\nOption[i64].Some(2)\nOption[i64].Some(0)\n3");
}

#[test]
fn float_search_uses_ieee_equality() {
    native(r#"
nan = 0.0 / 0.0
values = [nan, -0.0, 1.5, 0.0, nan]
print(values.count(nan))
print(values.find(nan))
print(values.rfind(nan))
print(values.count(0.0))
print(values.find(0.0))
print(values.rfind(-0.0))
print([0.0f32, -0.0f32].count(0.0f32))
"#, "0\nOption[i64].Nothing\nOption[i64].Nothing\n2\nOption[i64].Some(1)\nOption[i64].Some(3)\n2");
}

#[test]
fn query_operands_run_once_in_order_and_custom_methods_remain_available() {
    native(
        r#"
def items() -> list[i64]:
    print("receiver")
    [1, 2, 1]
def needle() -> i64:
    print("argument")
    1
print(items().find(needle()))
class Custom:
    def find(self) -> i64:
        42
print(Custom().find())
print("aba".find("a"))
"#,
        "receiver\nargument\nOption[i64].Some(0)\n42\nOption[i64].Some(0)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn queries_and_optional_results_allocate_nothing() {
    native(
        r#"
items = ["é" + "", "other", "é"]
query = "" + "é"
floats = [-0.0, 1.0, 0.0]
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
a = items.count(query)
b = items.find(query)
c = items.rfind("absent")
d = floats.count(0.0)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(a)
print(b)
print(c)
print(d)
"#,
        "2\nOption[i64].Some(0)\nOption[i64].Nothing\n2",
    );
}

#[rstest]
#[case("print([1].find())", "requires one element argument")]
#[case("print([1].rfind(1, 2))", "requires one element argument")]
#[case("print([1u8].count(1))", "expected u8")]
#[case("print([[1]].find([1]))", "list search supports")]
#[case("mut a = [1]\nr = &mut a\nprint(a.count(1))\nr.append(2)", "borrow")]
fn invalid_query(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

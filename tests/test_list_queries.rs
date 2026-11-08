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
    native(
        r#"
def locate(items: &list[str], query: &str) -> Option[i64]:
    items.find(query)
items = [("é" + "").unwrap(), "other", "é"].unwrap()
query = ("" + "é").unwrap()
print(locate(&items, &query)).unwrap()
print(items.rfind(query)).unwrap()
print(items.count(query)).unwrap()
print(items.find("missing")).unwrap()
print(list[i64]().unwrap().find(0)).unwrap()
print(list[i64]().unwrap().rfind(0)).unwrap()
print(list[i64]().unwrap().count(0)).unwrap()
print([False, True, False].unwrap().count(False)).unwrap()
print([1u8, 2u8, 1u8].unwrap().rfind(1u8)).unwrap()
print([-1, 2, -1].unwrap().find(-1)).unwrap()
print(len(items)).unwrap()
"#,
        "Option[i64].Some(0)\nOption[i64].Some(2)\n2\nOption[i64].Nothing\nOption[i64].Nothing\nOption[i64].Nothing\n0\n2\nOption[i64].Some(2)\nOption[i64].Some(0)\n3",
    );
}

#[test]
fn float_search_uses_ieee_equality() {
    native(
        r#"
nan = 0.0 / 0.0
values = [nan, -0.0, 1.5, 0.0, nan].unwrap()
print(values.count(nan)).unwrap()
print(values.find(nan)).unwrap()
print(values.rfind(nan)).unwrap()
print(values.count(0.0)).unwrap()
print(values.find(0.0)).unwrap()
print(values.rfind(-0.0)).unwrap()
print([0.0f32, -0.0f32].unwrap().count(0.0f32)).unwrap()
"#,
        "0\nOption[i64].Nothing\nOption[i64].Nothing\n2\nOption[i64].Some(1)\nOption[i64].Some(3)\n2",
    );
}

#[test]
fn query_operands_run_once_in_order_and_custom_methods_remain_available() {
    native(
        r#"
def items() -> list[i64]:
    print("receiver").unwrap()
    [1, 2, 1].unwrap()
def needle() -> i64:
    print("argument").unwrap()
    1
print(items().find(needle())).unwrap()
class Custom:
    def find(self) -> i64:
        42
print(Custom().find()).unwrap()
print("aba".find("a")).unwrap()
"#,
        "receiver\nargument\nOption[i64].Some(0)\n42\nOption[i64].Some(0)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn queries_and_optional_results_allocate_nothing() {
    native(
        r#"
items = [("é" + "").unwrap(), "other", "é"].unwrap()
query = ("" + "é").unwrap()
floats = [-0.0, 1.0, 0.0].unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a = items.count(query)
b = items.find(query)
c = items.rfind("absent")
d = floats.count(0.0)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
print(d).unwrap()
"#,
        "2\nOption[i64].Some(0)\nOption[i64].Nothing\n2",
    );
}

/// `count`, `find`, and `rfind` take every element type `in` takes, as a
/// slice view's do: other element types search the whole list's view with
/// the element type's `==` (issue #866).
#[test]
fn searches_take_every_membership_element_type() {
    native(
        r#"
class P:
    a: i64
    b: str

enum Shape:
    Dot
    Square(i64)

def last(items: &list[P]) -> Option[i64]:
    items.rfind(P(1, "x"))

def make() -> Result[list[list[i64]], AllocError]:
    [[1]?, [2, 3]?]

def main() -> Result[(), AllocError | IoError]:
    ps = [P(1, "x"), P(2, "y"), P(1, "x")]?
    print(ps.count(P(1, "x")), ps.find(P(2, "y")), ps.rfind(P(9, "z")), last(&ps))?
    shapes = [Shape.Dot, Shape.Square(2), Shape.Square(2)]?
    print(shapes.count(Shape.Square(2)), shapes.find(Shape.Dot))?
    inner = [2, 3]?
    print(make()?.find(inner), make()?.count(inner))?
    pairs = [(1, "a"), (2, "b")]?
    print(pairs.rfind((2, "b")))?
    empty: list[P] = []?
    print(empty.find(P(1, "x")))?
    Ok(())
"#,
        "2 Option[i64].Some(1) Option[i64].Nothing Option[i64].Some(2)\n2 Option[i64].Some(0)\nOption[i64].Some(1) 1\nOption[i64].Some(1)\nOption[i64].Nothing",
    );
}

#[rstest]
#[case("print([1].unwrap().find()).unwrap()", "requires one element argument")]
#[case(
    "print([1].unwrap().rfind(1, 2)).unwrap()",
    "requires one element argument"
)]
#[case(
    "value = 1\nprint([1u8].unwrap().count(value)).unwrap()",
    "expected u8"
)]
#[case(
    "hs = [Shared(1).unwrap()].unwrap()\nprint(hs.count(hs[0].share())).unwrap()",
    "values containing Shared owners do not support equality"
)]
#[case(
    "xs = [[1].unwrap()].unwrap()\nprint(xs.find(item=[1].unwrap())).unwrap()",
    "named arguments"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\nprint(a.count(1)).unwrap()\nr.append(2).unwrap()",
    "borrow"
)]
fn invalid_query(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

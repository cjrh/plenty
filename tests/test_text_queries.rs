//! Allocation-free text queries use literal UTF-8 and scalar positions.
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

#[rstest]
#[case("hello", "he", "lo", "True\nTrue")]
#[case("hello", "ell", "ell", "False\nFalse")]
#[case("", "", "", "True\nTrue")]
#[case("é\0🙂", "é\0", "\0🙂", "True\nTrue")]
#[case("abc", "abcd", "abcd", "False\nFalse")]
#[case("Abc", "a", "C", "False\nFalse")]
fn prefix_and_suffix_are_literal(
    #[case] text: &str,
    #[case] prefix: &str,
    #[case] suffix: &str,
    #[case] expected: &str,
) {
    native(
        &format!("print({text:?}.startswith({prefix:?}))\nprint({text:?}.endswith({suffix:?}))"),
        expected,
    );
}

#[test]
fn queries_observe_references_and_keep_class_methods() {
    native(
        r#"
def check(text: &str, prefix: &str) -> bool:
    text.startswith(prefix) and text.endswith(prefix)
class Custom:
    def startswith(self) -> i64:
        42
def text() -> str:
    print("receiver")
    return "aba"
def prefix() -> str:
    print("argument")
    return "a"
value = "a" + "ba"
part = "" + "a"
print(check(&value, &part))
print(value)
print(text().startswith(prefix()))
print(Custom().startswith())
"#,
        "True\naba\nreceiver\nargument\nTrue\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn prefix_and_suffix_checks_do_not_allocate() {
    native(
        r#"
text = "é\0" + "🙂"
prefix = "" + "é"
suffix = "" + "🙂"
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
a = text.startswith(prefix)
b = text.endswith(suffix)
c = text.startswith("")
d = text.endswith("missing")
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(a)
print(b)
print(c)
print(d)
"#,
        "True\nTrue\nTrue\nFalse",
    );
}

#[rstest]
#[case("print(\"a\".startswith())", "startswith requires one argument")]
#[case(
    "print(\"a\".endswith(\"a\", \"b\"))",
    "endswith requires one argument"
)]
#[case("print(\"a\".startswith(1))", "expected str")]
#[case("print([1].endswith(\"a\"))", "expected str")]
#[case(
    "mut text = \"a\"\nloan = &mut text\nprint(text.startswith(\"a\"))\n*loan = \"b\"",
    "borrow"
)]
fn invalid_prefix_queries(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[rstest]
#[case("banana", "ana", "Some(1)", "Some(3)")]
#[case("é🙂é🙂", "🙂", "Some(1)", "Some(3)")]
#[case("a\0b\0", "\0", "Some(1)", "Some(3)")]
#[case("abc", "x", "Nothing", "Nothing")]
#[case("", "", "Some(0)", "Some(0)")]
#[case("é🙂", "", "Some(0)", "Some(2)")]
#[case("abc", "abc", "Some(0)", "Some(0)")]
#[case("abc", "abcd", "Nothing", "Nothing")]
fn searches_return_optional_scalar_positions(
    #[case] text: &str,
    #[case] needle: &str,
    #[case] first: &str,
    #[case] last: &str,
) {
    native(
        &format!("print({text:?}.find({needle:?}))\nprint({text:?}.rfind({needle:?}))"),
        &format!("Option[i64].{first}\nOption[i64].{last}"),
    );
}

#[test]
fn search_references_propagation_and_inherent_methods() {
    native(r#"
def last(text: &str, needle: &str) -> Option[i64]:
    index = text.rfind(needle)?
    Some(index + 1)
class Custom:
    def find(self) -> i64:
        42
def source() -> str:
    print("receiver")
    return "aba"
def needle() -> str:
    print("argument")
    return "a"
text = "é" + "🙂"
part = "🙂"
print(last(&text, &part))
print(last(&text, &text))
print(text.find("missing"))
print(source().find(needle()))
print(Custom().find())
"#, "Option[i64].Some(2)\nOption[i64].Some(1)\nOption[i64].Nothing\nreceiver\nargument\nOption[i64].Some(0)\n42");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn search_results_and_empty_patterns_need_no_allocation() {
    native(
        r#"
text = "é🙂" + "é🙂"
needle = "" + "🙂"
print("__test_fail_allocations_after_0__")
print("__test_begin_no_allocations__")
a = text.find(needle)
b = text.rfind(needle)
c = text.rfind("")
d = text.find("missing")
drop(text)
drop(needle)
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
print(a)
print(b)
print(c)
print(d)
"#,
        "Option[i64].Some(1)\nOption[i64].Some(3)\nOption[i64].Some(4)\nOption[i64].Nothing",
    );
}

#[rstest]
#[case("print(\"a\".find())", "find requires one argument")]
#[case("print(\"a\".rfind(\"a\", 0))", "rfind requires one argument")]
#[case("print(\"a\".find(1))", "expected str")]
#[case(
    "mut text = \"a\"\nloan = &mut text\nprint(text.rfind(\"a\"))\n*loan = \"b\"",
    "borrow"
)]
fn invalid_search_queries(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

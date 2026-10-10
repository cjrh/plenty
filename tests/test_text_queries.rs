//! Allocation-free text queries use literal UTF-8 and scalar positions.
mod support;
use rstest::rstest;

#[rstest]
#[case("", "True\nFalse")]
#[case("hello", "True\nFalse")]
#[case(" \t\r\n", "True\nTrue")]
#[case("\0", "True\nFalse")]
#[case("é", "False\nFalse")]
#[case("🙂", "False\nFalse")]
#[case(" x ", "True\nFalse")]
fn classification_observes_all_characters(#[case] text: &str, #[case] expected: &str) {
    native(
        &format!("print({text:?}.isascii()).unwrap()\nprint({text:?}.isspace()).unwrap()"),
        expected,
    );
}

#[test]
fn whitespace_uses_the_same_unicode_property_as_stripping() {
    let source = r#"
def blank(text: &str) -> bool:
    text.isspace()
text = "UNICODE_SPACE"
print(blank(&text)).unwrap()
print(text.isascii()).unwrap()
print("ZERO_WIDTH_SPACE".isspace()).unwrap()
print("ZERO_WIDTH_SPACE".isascii()).unwrap()
class Custom:
    def isascii(self) -> i64:
        42
print(Custom().isascii()).unwrap()
def source() -> str:
    print("receiver").unwrap()
    " "
print(source().isspace()).unwrap()
"#
    .replace("UNICODE_SPACE", "\u{a0}\u{3000}")
    .replace("ZERO_WIDTH_SPACE", "\u{200b}");
    native(&source, "True\nFalse\nFalse\nFalse\n42\nreceiver\nTrue");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn classification_needs_no_allocation() {
    native(
        &r#"
spaces = (" " + "UNICODE_SPACE").unwrap()
text = ("hé" + "llo").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a = spaces.isspace()
b = spaces.isascii()
c = text.isascii()
d = text.isspace()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
print(d).unwrap()
"#
        .replace("UNICODE_SPACE", "\u{3000}"),
        "True\nFalse\nFalse\nFalse",
    );
}

#[rstest]
#[case("print(\"x\".isascii(1)).unwrap()", "takes no arguments")]
#[case("print(\"x\".isspace(\" \")).unwrap()", "takes no arguments")]
#[case("print([1].unwrap().isspace()).unwrap()", "expected str")]
#[case(
    "mut text = \"x\"\nr = &mut text\nprint(text.isascii()).unwrap()\n*r = \"y\"",
    "borrow"
)]
fn invalid_classification(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

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
        &format!("print({text:?}.startswith({prefix:?})).unwrap()\nprint({text:?}.endswith({suffix:?})).unwrap()"),
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
    print("receiver").unwrap()
    return "aba"
def prefix() -> str:
    print("argument").unwrap()
    return "a"
value = ("a" + "ba").unwrap()
part = ("" + "a").unwrap()
print(check(&value, &part)).unwrap()
print(value).unwrap()
print(text().startswith(prefix())).unwrap()
print(Custom().startswith()).unwrap()
"#,
        "True\naba\nreceiver\nargument\nTrue\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn prefix_and_suffix_checks_do_not_allocate() {
    native(
        r#"
text = ("é\0" + "🙂").unwrap()
prefix = ("" + "é").unwrap()
suffix = ("" + "🙂").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a = text.startswith(prefix)
b = text.endswith(suffix)
c = text.startswith("")
d = text.endswith("missing")
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
print(d).unwrap()
"#,
        "True\nTrue\nTrue\nFalse",
    );
}

#[rstest]
#[case(
    "print(\"a\".startswith()).unwrap()",
    "startswith requires one argument"
)]
#[case(
    "print(\"a\".endswith(\"a\", \"b\")).unwrap()",
    "endswith requires one argument"
)]
#[case("print(\"a\".startswith(1)).unwrap()", "expected str")]
#[case("print([1].unwrap().endswith(\"a\")).unwrap()", "expected str")]
#[case(
    "mut text = \"a\"\nloan = &mut text\nprint(text.startswith(\"a\")).unwrap()\n*loan = \"b\"",
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
        &format!(
            "print({text:?}.find({needle:?})).unwrap()\nprint({text:?}.rfind({needle:?})).unwrap()"
        ),
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
    print("receiver").unwrap()
    return "aba"
def needle() -> str:
    print("argument").unwrap()
    return "a"
text = ("é" + "🙂").unwrap()
part = "🙂"
print(last(&text, &part)).unwrap()
print(last(&text, &text)).unwrap()
print(text.find("missing")).unwrap()
print(source().find(needle())).unwrap()
print(Custom().find()).unwrap()
"#, "Option[i64].Some(2)\nOption[i64].Some(1)\nOption[i64].Nothing\nreceiver\nargument\nOption[i64].Some(0)\n42");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn search_results_and_empty_patterns_need_no_allocation() {
    native(
        r#"
text = ("é🙂" + "é🙂").unwrap()
needle = ("" + "🙂").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a = text.find(needle)
b = text.rfind(needle)
c = text.rfind("")
d = text.find("missing")
drop(text)
drop(needle)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
print(d).unwrap()
"#,
        "Option[i64].Some(1)\nOption[i64].Some(3)\nOption[i64].Some(4)\nOption[i64].Nothing",
    );
}

#[rstest]
#[case("print(\"a\".find()).unwrap()", "find requires one argument")]
#[case("print(\"a\".rfind(\"a\", 0)).unwrap()", "rfind requires one argument")]
#[case("print(\"a\".find(1)).unwrap()", "expected str")]
#[case(
    "mut text = \"a\"\nloan = &mut text\nprint(text.rfind(\"a\")).unwrap()\n*loan = \"b\"",
    "borrow"
)]
fn invalid_search_queries(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[rstest]
#[case("banana", "ana", 1)]
#[case("aaaaa", "aa", 2)]
#[case("é🙂é🙂", "🙂", 2)]
#[case("a\0b\0", "\0", 2)]
#[case("abc", "x", 0)]
#[case("", "", 1)]
#[case("é🙂", "", 3)]
#[case("abc", "abc", 1)]
fn counts_non_overlapping_literal_matches(
    #[case] text: &str,
    #[case] needle: &str,
    #[case] expected: i64,
) {
    native(
        &format!("print({text:?}.count({needle:?})).unwrap()"),
        &expected.to_string(),
    );
}

#[test]
fn counting_borrows_inputs_and_keeps_inherent_method_dispatch() {
    native(
        r#"
def count(text: &str, needle: &str) -> i64:
    text.count(needle)
class Custom:
    def count(self) -> i64:
        42
def source() -> str:
    print("receiver").unwrap()
    return "aba"
def needle() -> str:
    print("argument").unwrap()
    return "a"
text = ("a" + "ba").unwrap()
part = ("" + "a").unwrap()
print(count(&text, &part)).unwrap()
print(text).unwrap()
print(source().count(needle())).unwrap()
print(Custom().count()).unwrap()
"#,
        "2\naba\nreceiver\nargument\n2\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn counting_and_empty_pattern_handling_allocate_nothing() {
    native(
        r#"
text = ("é🙂" + "é🙂").unwrap()
needle = ("" + "🙂").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a = text.count(needle)
b = text.count("")
c = text.count("missing")
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
"#,
        "2\n5\n0",
    );
}

#[rstest]
#[case("print(\"a\".count()).unwrap()", "count requires one argument")]
#[case("print(\"a\".count(\"a\", 0)).unwrap()", "count requires one argument")]
#[case("print(\"a\".count(1)).unwrap()", "expected str")]
fn invalid_count_queries(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

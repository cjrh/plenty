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

//! Literal replacement with checked output lengths and recoverable allocation.
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
#[case("banana", "na", "!", "ba!!")]
#[case("aaaaa", "aa", "X", "XXa")]
#[case("ababa", "aba", "X", "Xba")]
#[case("abc", "x", "y", "abc")]
#[case("abc", "abc", "", "")]
#[case("abc", "b", "bbb", "abbbc")]
#[case("abc", "b", "b", "abc")]
#[case("", "x", "y", "")]
#[case("", "", "X", "X")]
#[case("", "", "", "")]
#[case("ab", "", "-", "-a-b-")]
#[case("é🙂", "", "中", "中é中🙂中")]
#[case("é🙂é", "é", "🙂Z", "🙂Z🙂🙂Z")]
#[case("a\0b\0", "\0", "é", "aébé")]
fn replacement_is_literal_and_non_overlapping(
    #[case] text: &str,
    #[case] old: &str,
    #[case] new: &str,
    #[case] expected: &str,
) {
    native(
        &format!("print({text:?}.try_replace({old:?}, {new:?}))"),
        &format!("Result[str, AllocError].Ok({expected:?})"),
    );
}

#[test]
fn replacement_observes_fields_and_reference_arguments_and_preserves_lifetimes() {
    native(
        r#"
class Message:
    text: str
def replace(text: &str, old: &str, new: &str) -> Result[str, AllocError]:
    text.try_replace(old, new)
mut message = Message("Aé" + "🙂é")
old = "" + "é"
new = "" + "Z"
saved = replace(&message.text, &old, &new)
message.text = "changed"
drop(message)
drop(old)
drop(new)
print(saved)
def inspect(text: &mut str) -> Result[i64, AllocError]:
    result = text.try_replace("", "🙂")?
    *text = "changed"
    print(result)
    Ok(len(result))
mut text = "ab"
print(inspect(&mut text))
print(text)
"#,
        "Result[str, AllocError].Ok(\"AZ🙂Z\")\n🙂a🙂b🙂\nResult[i64, AllocError].Ok(5)\nchanged",
    );
}

#[test]
fn replacement_operands_evaluate_once_in_order_and_propagate_before_later_operands() {
    native(r#"
def text(label: str, value: str) -> str:
    print(label)
    value
def missing() -> Result[str, AllocError]:
    print("missing")
    Err(AllocError.CapacityOverflow)
def fail() -> Result[str, AllocError]:
    text("receiver", "A" + "B").try_replace(missing()?, text("skipped", "C"))
print(text("receiver", "aba").try_replace(text("old", "a"), text("new", "X")))
print(fail())
class Custom:
    def try_replace(self, value: i64) -> i64:
        value
print(Custom().try_replace(42))
"#, "receiver\nold\nnew\nResult[str, AllocError].Ok(\"XbX\")\nreceiver\nmissing\nResult[str, AllocError].Err(AllocError.CapacityOverflow)\n42");
}

#[test]
fn replacement_accepts_shared_input_storage_without_consuming_it() {
    native(
        r#"
text = "a" + "b"
print(text.try_replace(text, text))
print(text.try_replace("", text))
print(text)
"#,
        "Result[str, AllocError].Ok(\"ab\")\nResult[str, AllocError].Ok(\"abaabbab\")\nab",
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("é", "🙂", "A🙂🙂Z")]
#[case("", "X", "XAXéXéXZX")]
#[case("missing", "X", "AééZ")]
#[case("AééZ", "", "")]
fn only_the_final_string_allocation_can_fail(
    #[case] old: &str,
    #[case] new: &str,
    #[case] expected: &str,
) {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
text = "Aé" + "éZ"
old = {old:?}
new = {new:?}
print("__test_fail_allocations_after_{budget}__")
result = text.try_replace(old, new)
print("__test_restore_allocations__")
print(result)
print(text)
print(text.try_replace(old, new))
"#
            ),
            &format!(
                "Result[str, AllocError].{}\nAééZ\nResult[str, AllocError].Ok({expected:?})",
                if budget == 0 {
                    "Err(AllocError.OutOfMemory)".to_owned()
                } else {
                    format!("Ok({expected:?})")
                }
            ),
        );
    }
}

#[rstest]
#[case(
    "print(\"text\".try_replace())",
    "try_replace requires old and new string arguments"
)]
#[case(
    "print(\"text\".try_replace(\"t\"))",
    "try_replace requires old and new string arguments"
)]
#[case(
    "print(\"text\".try_replace(\"t\", \"X\", 1))",
    "try_replace requires old and new string arguments"
)]
#[case("print(\"text\".try_replace(1, \"X\"))", "expected str")]
#[case("print(\"text\".try_replace(\"t\", 1))", "expected str")]
#[case("print([1].try_replace(\"t\", \"X\"))", "expected str")]
#[case("mut text = \"text\"\nloan = &mut text\nprint(text.try_replace(\"t\", \"X\"))\n*loan = \"changed\"", "borrow")]
fn invalid_replacement_is_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

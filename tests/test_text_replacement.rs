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
        &format!("print(str.repr({text:?}.replace({old:?}, {new:?})).unwrap()).unwrap()"),
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
    text.replace(old, new)
mut message = Message(("Aé" + "🙂é").unwrap())
old = ("" + "é").unwrap()
new = ("" + "Z").unwrap()
saved = replace(&message.text, &old, &new)
message.text = "changed"
drop(message)
drop(old)
drop(new)
print(str.repr(saved).unwrap()).unwrap()
def inspect(text: &mut str) -> Result[i64, AllocError]:
    result = text.replace("", "🙂")?
    *text = "changed"
    print(result).unwrap()
    Ok(len(result))
mut text = "ab"
print(str.repr(inspect(&mut text)).unwrap()).unwrap()
print(text).unwrap()
"#,
        "Result[str, AllocError].Ok(\"AZ🙂Z\")\n🙂a🙂b🙂\nResult[i64, AllocError].Ok(5)\nchanged",
    );
}

#[test]
fn replacement_operands_evaluate_once_in_order_and_propagate_before_later_operands() {
    native(r#"
def text(label: str, value: str) -> str:
    print(label).unwrap()
    value
def missing() -> Result[str, AllocError]:
    print("missing").unwrap()
    Err(AllocError.CapacityOverflow)
def fail() -> Result[str, AllocError]:
    text("receiver", ("A" + "B").unwrap()).replace(missing()?, text("skipped", "C"))
print(str.repr(text("receiver", "aba").replace(text("old", "a"), text("new", "X"))).unwrap()).unwrap()
print(str.repr(fail()).unwrap()).unwrap()
class Custom:
    def replace(self, value: i64) -> i64:
        value
print(Custom().replace(42)).unwrap()
"#, "receiver\nold\nnew\nResult[str, AllocError].Ok(\"XbX\")\nreceiver\nmissing\nResult[str, AllocError].Err(AllocError.CapacityOverflow)\n42");
}

#[test]
fn replacement_accepts_shared_input_storage_without_consuming_it() {
    native(
        r#"
text = ("a" + "b").unwrap()
print(str.repr(text.replace(text, text)).unwrap()).unwrap()
print(str.repr(text.replace("", text)).unwrap()).unwrap()
print(text).unwrap()
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
text = ("Aé" + "éZ").unwrap()
old = {old:?}
new = {new:?}
print("__test_fail_allocations_after_{budget}__").unwrap()
result = text.replace(old, new)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(text).unwrap()
print(str.repr(text.replace(old, new)).unwrap()).unwrap()
"#
            ),
            &format!(
                "Result[str, AllocError].{}\nAééZ\nResult[str, AllocError].Ok({expected:?})",
                if budget == 0 && expected.len() > support::INLINE_MAX {
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
    "print(\"text\".replace()).unwrap()",
    "replace requires old and new string arguments"
)]
#[case(
    "print(\"text\".replace(\"t\")).unwrap()",
    "replace requires old and new string arguments"
)]
#[case(
    "print(\"text\".replace(\"t\", \"X\", 1)).unwrap()",
    "replace requires old and new string arguments"
)]
#[case("print(\"text\".replace(1, \"X\")).unwrap()", "expected str")]
#[case("print(\"text\".replace(\"t\", 1)).unwrap()", "expected str")]
#[case("print([1].unwrap().replace(\"t\", \"X\")).unwrap()", "expected str")]
#[case("mut text = \"text\"\nloan = &mut text\nprint(str.repr(text.replace(\"t\", \"X\")).unwrap()).unwrap()\n*loan = \"changed\"", "borrow")]
fn invalid_replacement_is_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

//! Fallible text transformations keep immutable inputs and allocate final outputs.
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
#[case("try_strip", "  a b  ", "a b")]
#[case("try_lstrip", "  a b  ", "a b  ")]
#[case("try_rstrip", "  a b  ", "  a b")]
#[case("try_strip", "\t\r\né🙂\n\t", "é🙂")]
#[case("try_strip", "", "")]
#[case("try_strip", " \t\n", "")]
#[case("try_lstrip", "x", "x")]
#[case("try_rstrip", "x", "x")]
#[case("try_strip", " \0 ", "\0")]
fn strip_keeps_inner_text(#[case] method: &str, #[case] text: &str, #[case] expected: &str) {
    native(
        &format!("print({text:?}.{method}())"),
        &format!("Result[str, AllocError].Ok({expected:?})"),
    );
}

#[test]
fn stripping_uses_unicode_whitespace_and_borrows_references() {
    native(&r#"
def strip(text: &str) -> Result[str, AllocError]:
    result = text.try_strip()?
    Ok(result)
text = "　 é🙂 　"
print(strip(&text))
print(text.try_lstrip())
print(text.try_rstrip())
print(len(text))
print(len("ZERO_WIDTH_SPACE"))
print("ZERO_WIDTH_SPACE".try_strip())
class Custom:
    def try_strip(self, value: i64) -> i64:
        value
print(Custom().try_strip(42))
"#.replace("ZERO_WIDTH_SPACE", "\u{200b}"), "Result[str, AllocError].Ok(\"é🙂\")\nResult[str, AllocError].Ok(\"é🙂 　\")\nResult[str, AllocError].Ok(\"　 é🙂\")\n6\n1\nResult[str, AllocError].Ok(\"\u{200b}\")\n42");
}

#[test]
fn stripped_text_outlives_replaced_source_and_receiver_runs_once() {
    native(
        r#"
def source() -> str:
    print("receiver")
    return " x "
mut text = " é" + "🙂 "
saved = text.try_strip()
text = "changed"
drop(text)
print(saved)
print(source().try_strip())
"#,
        "Result[str, AllocError].Ok(\"é🙂\")\nreceiver\nResult[str, AllocError].Ok(\"x\")",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn strip_has_one_recoverable_output_allocation_in_all_cases() {
    for (text, expected) in [
        ("  é🙂  ", "é🙂"),
        ("unchanged", "unchanged"),
        ("", ""),
        ("   ", ""),
    ] {
        for budget in 0..=1 {
            native(
                &format!(
                    r#"
text = {text:?}
print("__test_fail_allocations_after_{budget}__")
result = text.try_strip()
print("__test_restore_allocations__")
print(result)
print(text.try_strip())
"#
                ),
                &format!(
                    "Result[str, AllocError].{}\nResult[str, AllocError].Ok({expected:?})",
                    if budget == 0 {
                        "Err(AllocError.OutOfMemory)".to_owned()
                    } else {
                        format!("Ok({expected:?})")
                    }
                ),
            );
        }
    }
}

#[rstest]
#[case("print(\"x\".try_strip(\"x\"))", "try_strip takes no arguments")]
#[case("print(\"x\".try_lstrip(1))", "try_lstrip takes no arguments")]
#[case("print([1].try_rstrip())", "expected str")]
#[case(
    "mut text = \"a\"\nloan = &mut text\nprint(text.try_strip())\n*loan = \"b\"",
    "borrow"
)]
fn invalid_strip(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

//! Fallible text transformations keep immutable inputs and allocate final outputs.
mod support;
use rstest::rstest;

#[test]
fn splitlines_preserves_optional_terminators_and_independent_results() {
    native(r#"
def lines(text: &str) -> Result[list[str], AllocError]:
    text.splitlines()
mut text = ("é\r\n\n🦀\rlast" + "").unwrap()
result = lines(&text)
print(text.splitlines(True)).unwrap()
text = "changed"
drop(text)
print(result).unwrap()
print("".splitlines()).unwrap()
print("\n".splitlines()).unwrap()
print("a\nb\n".splitlines()).unwrap()
print("a\0b".splitlines()).unwrap()
"#, "Result[list[str], AllocError].Ok([\"é\\r\\n\", \"\\n\", \"🦀\\r\", \"last\"])\nResult[list[str], AllocError].Ok([\"é\", \"\", \"🦀\", \"last\"])\nResult[list[str], AllocError].Ok([])\nResult[list[str], AllocError].Ok([\"\"])\nResult[list[str], AllocError].Ok([\"a\", \"b\"])\nResult[list[str], AllocError].Ok([\"a\\0b\"])");
    for source in [
        "print(\"x\".splitlines(1)).unwrap()",
        "print(\"x\".splitlines(True, False)).unwrap()",
        "print([1].unwrap().splitlines()).unwrap()",
    ] {
        assert!(support::check_source(source).is_err());
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn splitlines_recovers_from_each_allocation_and_preserves_source() {
    for budget in 0..=5 {
        native(
            &format!(
                r#"
source = ("firstline\nsecondline\nthirdline" + "").unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = source.splitlines()
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(len(source)).unwrap()
"#
            ),
            &format!(
                "Result[list[str], AllocError].{}\n30",
                if budget < 5 {
                    "Err(AllocError.OutOfMemory)"
                } else {
                    "Ok([\"firstline\", \"secondline\", \"thirdline\"])"
                }
            ),
        );
    }
}

#[rstest]
#[case("removeprefix", "abab", "ab", "ab")]
#[case("removesuffix", "abab", "ab", "ab")]
#[case("removeprefix", "é🙂é", "é", "🙂é")]
#[case("removesuffix", "é🙂é", "é", "é🙂")]
#[case("removeprefix", "\0x", "\0", "x")]
#[case("removesuffix", "x\0", "\0", "x")]
#[case("removeprefix", "abc", "bc", "abc")]
#[case("removesuffix", "abc", "ab", "abc")]
#[case("removeprefix", "abc", "abcd", "abc")]
#[case("removesuffix", "abc", "abc", "")]
#[case("removeprefix", "", "", "")]
#[case("removesuffix", "abc", "", "abc")]
fn affix_removal_is_literal(
    #[case] method: &str,
    #[case] source: &str,
    #[case] pattern: &str,
    #[case] expected: &str,
) {
    native(
        &format!("print({source:?}.{method}({pattern:?})).unwrap()"),
        &format!("Result[str, AllocError].Ok({expected:?})"),
    );
}

#[test]
fn affix_arguments_are_observed_in_order_and_results_outlive_inputs() {
    native(r#"
def source() -> str:
    print("source").unwrap()
    "pre-value"
def pattern() -> str:
    print("pattern").unwrap()
    "pre-"
def trim(text: &str, prefix: &str) -> Result[str, AllocError]:
    text.removeprefix(prefix)?.removesuffix("-end")
print(source().removeprefix(pattern())).unwrap()
mut text = ("pre-é" + "-end").unwrap()
prefix = ("pre" + "-").unwrap()
result = trim(&text, &prefix)
text = "changed"
drop(text)
drop(prefix)
print(result).unwrap()
class Custom:
    def removeprefix(self) -> i64:
        42
print(Custom().unwrap().removeprefix()).unwrap()
"#, "source\npattern\nResult[str, AllocError].Ok(\"value\")\nResult[str, AllocError].Ok(\"é\")\n42");
}

#[cfg(feature = "runtime-checks")]
#[rstest]
fn affix_removal_recovers_from_its_single_output_allocation(
    #[values("removeprefix", "removesuffix")] method: &str,
    #[values("é", "missing", "")] pattern: &str,
    #[values(0, 1)] budget: usize,
) {
    let expected = match (method, pattern) {
        ("removeprefix", "é") => "légante",
        _ => "élégante",
    };
    native(
        &format!(
            r#"
source = ("" + "élégante").unwrap()
pattern = ("" + {pattern:?}).unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = source.{method}(pattern)
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(source).unwrap()
"#
        ),
        &format!(
            "Result[str, AllocError].{}\nélégante",
            if budget == 0 {
                "Err(AllocError.OutOfMemory)".to_owned()
            } else {
                format!("Ok({expected:?})")
            }
        ),
    );
}

#[rstest]
#[case("print(\"x\".removeprefix()).unwrap()", "requires one argument")]
#[case("print(\"x\".removesuffix(1)).unwrap()", "expected str")]
#[case("print([1].unwrap().removeprefix(\"x\")).unwrap()", "expected str")]
fn invalid_affix_removal(#[case] source: &str, #[case] expected: &str) {
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
#[case("strip", "  a b  ", "a b")]
#[case("lstrip", "  a b  ", "a b  ")]
#[case("rstrip", "  a b  ", "  a b")]
#[case("strip", "\t\r\né🙂\n\t", "é🙂")]
#[case("strip", "", "")]
#[case("strip", " \t\n", "")]
#[case("lstrip", "x", "x")]
#[case("rstrip", "x", "x")]
#[case("strip", " \0 ", "\0")]
fn strip_keeps_inner_text(#[case] method: &str, #[case] text: &str, #[case] expected: &str) {
    native(
        &format!("print({text:?}.{method}()).unwrap()"),
        &format!("Result[str, AllocError].Ok({expected:?})"),
    );
}

#[test]
fn stripping_uses_unicode_whitespace_and_borrows_references() {
    native(&r#"
def strip(text: &str) -> Result[str, AllocError]:
    result = text.strip()?
    Ok(result)
text = "　 é🙂 　"
print(strip(&text)).unwrap()
print(text.lstrip()).unwrap()
print(text.rstrip()).unwrap()
print(len(text)).unwrap()
print(len("ZERO_WIDTH_SPACE")).unwrap()
print("ZERO_WIDTH_SPACE".strip()).unwrap()
class Custom:
    def strip(self, value: i64) -> i64:
        value
print(Custom().unwrap().strip(42)).unwrap()
"#.replace("ZERO_WIDTH_SPACE", "\u{200b}"), "Result[str, AllocError].Ok(\"é🙂\")\nResult[str, AllocError].Ok(\"é🙂 　\")\nResult[str, AllocError].Ok(\"　 é🙂\")\n6\n1\nResult[str, AllocError].Ok(\"\u{200b}\")\n42");
}

#[test]
fn stripped_text_outlives_replaced_source_and_receiver_runs_once() {
    native(
        r#"
def source() -> str:
    print("receiver").unwrap()
    return " x "
mut text = (" é" + "🙂 ").unwrap()
saved = text.strip()
text = "changed"
drop(text)
print(saved).unwrap()
print(source().strip()).unwrap()
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
print("__test_fail_allocations_after_{budget}__").unwrap()
result = text.strip()
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(text.strip()).unwrap()
"#
                ),
                &format!(
                    "Result[str, AllocError].{}\nResult[str, AllocError].Ok({expected:?})",
                    if budget == 0 && expected.len() > support::INLINE_MAX {
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
#[case("print(\"x\".strip(\"x\")).unwrap()", "strip takes no arguments")]
#[case("print(\"x\".lstrip(1)).unwrap()", "lstrip takes no arguments")]
#[case("print([1].unwrap().rstrip()).unwrap()", "expected str")]
#[case(
    "mut text = \"a\"\nloan = &mut text\nprint(text.strip()).unwrap()\n*loan = \"b\"",
    "borrow"
)]
fn invalid_strip(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[rstest]
#[case("ab", "3", "ababab")]
#[case("é\0🙂", "2", "é\0🙂é\0🙂")]
#[case("ab", "1", "ab")]
#[case("ab", "0", "")]
#[case("ab", "-1", "")]
#[case("ab", "-9223372036854775808", "")]
#[case("", "9223372036854775807", "")]
fn repeat_handles_counts_and_exact_utf8(
    #[case] text: &str,
    #[case] count: &str,
    #[case] expected: &str,
) {
    native(
        &format!("print({text:?}.repeat({count})).unwrap()"),
        &format!("Result[str, AllocError].Ok({expected:?})"),
    );
}

#[test]
fn repeat_borrows_operands_and_outlives_source_changes() {
    native(r#"
def repeat(text: &str, count: &i64) -> Result[str, AllocError]:
    output = text.repeat(count)?
    Ok(output)
mut text = ("é" + "🙂").unwrap()
count = 3
saved = repeat(&text, &count)
text = "changed"
drop(text)
print(saved).unwrap()
class Custom:
    def repeat(self) -> i64:
        42
print(Custom().unwrap().repeat()).unwrap()
def source() -> str:
    print("receiver").unwrap()
    return "x"
def copies() -> i64:
    print("count").unwrap()
    2
print(source().repeat(copies())).unwrap()
"#, "Result[str, AllocError].Ok(\"é🙂é🙂é🙂\")\n42\nreceiver\ncount\nResult[str, AllocError].Ok(\"xx\")");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn repetition_has_one_output_allocation_and_checked_capacity() {
    for (count, expected) in [(3, "é🙂é🙂é🙂"), (0, ""), (-1, "")] {
        for budget in 0..=1 {
            native(
                &format!(
                    r#"
text = ("é" + "🙂").unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = text.repeat({count})
print("__test_restore_allocations__").unwrap()
print(result).unwrap()
print(text).unwrap()
print(text.repeat({count})).unwrap()
"#
                ),
                &format!(
                    "Result[str, AllocError].{}\né🙂\nResult[str, AllocError].Ok({expected:?})",
                    if budget == 0 && expected.len() > support::INLINE_MAX {
                        "Err(AllocError.OutOfMemory)".to_owned()
                    } else {
                        format!("Ok({expected:?})")
                    }
                ),
            );
        }
    }
    native(r#"
print("__test_fail_allocations_after_0__").unwrap()
print("__test_begin_no_allocations__").unwrap()
a = "x".repeat(9223372036854775807)
b = "é🙂".repeat(9223372036854775807)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
"#, "Result[str, AllocError].Err(AllocError.CapacityOverflow)\nResult[str, AllocError].Err(AllocError.CapacityOverflow)");
}

#[rstest]
#[case("print(\"x\".repeat()).unwrap()", "repeat requires one argument")]
#[case("print(\"x\".repeat(2u8)).unwrap()", "expected i64")]
#[case("print([1].unwrap().repeat(2)).unwrap()", "expected str")]
fn invalid_repeat(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

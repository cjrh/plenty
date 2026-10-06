//! Length-aware UTF-8 building with recoverable allocation failure.
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

#[test]
fn concat_and_join_preserve_utf8_nul_and_scalar_lengths() {
    native(
        r#"
def show() -> Result[(), AllocError]:
    combined = "é\0".try_concat("🙂")?
    parts = ["é", "", "🙂\0"]
    joined = "界".try_join(parts)?
    print(len(combined))
    print(combined == "é\0🙂")
    print(len(joined))
    print(joined == "é界界🙂\0")
    print(len(parts))
    print("-".try_join([])?)
    print("-".try_join(["one"])?)
    print("".try_join(["a", "b"])?)
    print("".try_concat("")?)
    Ok(())
print(show())
"#,
        "3\nTrue\n5\nTrue\n3\n\none\nab\n\nResult[(), AllocError].Ok(())",
    );
}

#[test]
fn borrowed_arguments_fields_and_chained_results_remain_usable() {
    native(
        r#"
class Texts:
    prefix: str
    parts: list[str]
def format(prefix: &str, parts: &list[str]) -> Result[str, AllocError]:
    prefix.try_concat(", ".try_join(parts)?)?.try_concat("!")
def show(data: &Texts) -> Result[str, AllocError]:
    format(&data.prefix, &data.parts)
data = Texts("Hello ", ["Ada", "Bea"])
print(show(&data))
print(data.prefix)
print(data.parts)
"#,
        "Result[str, AllocError].Ok(\"Hello Ada, Bea!\")\nHello \n[\"Ada\", \"Bea\"]",
    );
}

#[test]
fn receiver_and_argument_are_evaluated_once_in_order() {
    native(
        r#"
def separator() -> str:
    print("separator")
    "-"
def parts() -> list[str]:
    print("parts")
    ["a", "b"]
class Custom:
    def try_join(self, text: str) -> str:
        text
print(separator().try_join(parts()))
print(Custom().try_join("class method"))
"#,
        "separator\nparts\nResult[str, AllocError].Ok(\"a-b\")\nclass method",
    );
}

#[test]
fn split_preserves_empty_fields_unicode_and_embedded_nuls() {
    native(
        r#"
def check() -> Result[(), AllocError]:
    pieces = "::é\0::🙂::::".try_split("::")?
    print(pieces)
    print(len(pieces[1]))
    print("::".try_join(pieces)? == "::é\0::🙂::::")
    print("".try_split(",")?)
    print("abc".try_split("absent")?)
    print("aaaaa".try_split("aa")?)
    print("a界b界".try_split("界")?)
    print("a\0b\0".try_split("\0")?)
    Ok(())
print(check())
"#,
        "[\"\", \"é\\0\", \"🙂\", \"\", \"\"]\n2\nTrue\n[\"\"]\n[\"abc\"]\n[\"\", \"\", \"a\"]\n[\"a\", \"b\", \"\"]\n[\"a\", \"b\", \"\"]\nResult[(), AllocError].Ok(())",
    );
}

#[test]
fn splitting_borrows_fields_and_parameters_and_evaluates_once() {
    native(
        r#"
class Text:
    contents: str
def split(text: &str, separator: &str) -> Result[list[str], AllocError]:
    text.try_split(separator)
def source() -> str:
    print("source")
    "left:right"
def separator() -> str:
    print("separator")
    ":"
class Custom:
    def try_split(self, text: str) -> str:
        text
text = Text("a:b")
sep = ":"
print(split(&text.contents, &sep))
print(text.contents)
print(source().try_split(separator()))
print(Custom().try_split("custom"))
"#,
        "Result[list[str], AllocError].Ok([\"a\", \"b\"])\na:b\nsource\nseparator\nResult[list[str], AllocError].Ok([\"left\", \"right\"])\ncustom",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn splitting_recovers_at_every_allocation_and_preserves_sources() {
    // One entry buffer, one list owner, and three piece allocations.
    for budget in 0..=5 {
        native(
            &format!(r#"
source = "a:" + "b:"
separator = "" + ":"
print("__test_fail_allocations_after_{budget}__")
result = source.try_split(separator)
print("__test_restore_allocations__")
print(result)
print(source)
print(separator)
print(source.try_split(separator))
"#),
            &format!("Result[list[str], AllocError].{}\na:b:\n:\nResult[list[str], AllocError].Ok([\"a\", \"b\", \"\"])",
                if budget < 5 { "Err(AllocError.OutOfMemory)" } else { "Ok([\"a\", \"b\", \"\"])" }),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_split_propagates_and_cleans_partial_output_without_allocating() {
    for budget in 0..5 {
        native(
            &format!(
                r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped")
def split(source: str, guard: Guard) -> Result[list[str], AllocError]:
    pieces = source.try_split(":")?
    print("unreachable")
    Ok(pieces)
source = "a:" + "b:"
guard = Guard()
print("__test_fail_allocations_after_{budget}__")
result = split(source, guard)
print("__test_begin_no_allocations__")
match result:
    case Ok(pieces):
        print("unexpected success")
    case Err(error):
        print("handled")
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
"#
            ),
            "dropped\nhandled",
        );
    }
}

#[test]
fn split_rejects_empty_separator_at_runtime() {
    let output = support::run("separator = \"\"\nprint(\"abc\".try_split(separator))");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("string split requires a nonempty separator"));
}

#[test]
fn checked_character_lookup_uses_scalar_indices_and_handles_extreme_bounds() {
    native(
        r#"
def show(text: &str, index: i64) -> Result[(), AllocError]:
    print(text.try_get(index)?)
    Ok(())
text = "é🙂\0"
for index in [-4, -3, -2, -1, 0, 1, 2, 3, -9223372036854775808, 9223372036854775807]:
    show(&text, index)
empty = ""
show(&empty, 0)
print(text == "é🙂\0")
"#,
        "Option[str].Nothing\nOption[str].Some(\"é\")\nOption[str].Some(\"🙂\")\nOption[str].Some(\"\\0\")\nOption[str].Some(\"é\")\nOption[str].Some(\"🙂\")\nOption[str].Some(\"\\0\")\nOption[str].Nothing\nOption[str].Nothing\nOption[str].Nothing\nOption[str].Nothing\nTrue",
    );
}

#[test]
fn character_lookup_borrows_fields_and_evaluates_operands_once() {
    native(
        r#"
class Text:
    value: str
def source() -> str:
    print("source")
    "é🙂"
def index() -> i64:
    print("index")
    -1
class Custom:
    def try_get(self, index: i64) -> i64:
        index
text = Text("abc")
position = -2
print(text.value.try_get(&position))
print(text.value)
print(source().try_get(index()))
print(Custom().try_get(42))
"#,
        "Result[Option[str], AllocError].Ok(Option[str].Some(\"b\"))\nabc\nsource\nindex\nResult[Option[str], AllocError].Ok(Option[str].Some(\"🙂\"))\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn missing_character_needs_no_allocation_and_present_character_needs_one() {
    for budget in 0..=1 {
        native(
            &format!(r#"
text = "é" + "🙂"
print("__test_fail_allocations_after_{budget}__")
print("__test_begin_no_allocations__")
missing = text.try_get(-3)
print("__test_end_no_allocations__")
present = text.try_get(-1)
print("__test_restore_allocations__")
print(missing)
print(present)
print(text)
print(text.try_get(-1))
"#),
            &format!("Result[Option[str], AllocError].Ok(Option[str].Nothing)\nResult[Option[str], AllocError].{}\né🙂\nResult[Option[str], AllocError].Ok(Option[str].Some(\"🙂\"))", if budget == 0 { "Err(AllocError.OutOfMemory)" } else { "Ok(Option[str].Some(\"🙂\"))" }),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn character_allocation_failure_propagates_and_drops_locals() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped")
def lookup(text: str, guard: Guard) -> Result[Option[str], AllocError]:
    character = text.try_get(0)?
    print("unreachable")
    Ok(character)
text = "é" + "🙂"
guard = Guard()
print("__test_fail_allocations_after_0__")
result = lookup(text, guard)
print("__test_begin_no_allocations__")
match result:
    case Ok(character):
        print("unexpected success")
    case Err(error):
        print("handled")
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
"#,
        "dropped\nhandled",
    );
}

#[test]
fn index_expression_can_propagate_before_character_allocation() {
    native(
        r#"
def missing_index() -> Result[i64, AllocError]:
    print("index")
    Err(AllocError.CapacityOverflow)
def lookup() -> Result[Option[str], AllocError]:
    ("a" + "b").try_get(missing_index()?)
print(lookup())
"#,
        "index\nResult[Option[str], AllocError].Err(AllocError.CapacityOverflow)",
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("left.try_concat(right)", "left-right")]
#[case("separator.try_join(parts)", "left|right")]
#[case("separator.try_join(empty)", "")]
fn one_output_allocation_suffices_and_failure_can_be_retried(
    #[case] expression: &str,
    #[case] expected: &str,
) {
    for budget in 0..=1 {
        native(&format!(r#"
left = "le" + "ft"
right = "-ri" + "ght"
separator = "|"
parts = ["le" + "ft", "ri" + "ght"]
empty = list[str]()
print("__test_fail_allocations_after_{budget}__")
result = {expression}
print("__test_restore_allocations__")
print(result)
print({expression})
print(left)
print(right)
print(parts)
"#), &format!("Result[str, AllocError].{}\nResult[str, AllocError].Ok(\"{expected}\")\nleft\n-right\n[\"left\", \"right\"]", if budget == 0 { "Err(AllocError.OutOfMemory)".into() } else { format!("Ok(\"{expected}\")") }));
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_join_releases_temporary_inputs_and_propagates_without_allocating() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped")
def build(parts: list[str], guard: Guard) -> Result[str, AllocError]:
    joined = "-".try_join(parts)?
    print("unreachable")
    Ok(joined)
parts = ["ab" + "cd", "ef" + "gh"]
guard = Guard()
print("__test_fail_allocations_after_0__")
result = build(parts, guard)
print("__test_begin_no_allocations__")
match result:
    case Ok(text):
        print("unexpected success")
    case Err(error):
        print("handled")
print("__test_restore_allocations__")
print("__test_end_no_allocations__")
"#,
        "dropped\nhandled",
    );
}

#[rstest]
#[case("print(\"a\".try_concat())", "try_concat requires one argument")]
#[case("print(\"a\".try_join([], []))", "try_join requires one argument")]
#[case("print(\"a\".try_concat(1))", "expected str")]
#[case("print(\"a\".try_join([1, 2]))", "expected list[str]")]
#[case(
    "print(\"a\".try_join({\"x\"}))",
    "collection type does not match its annotation"
)]
#[case("print((1).try_concat(\"a\"))", "expected str")]
#[case("print(\"a\".try_split())", "try_split requires one argument")]
#[case("print(\"a\".try_split(\",\", 1))", "try_split requires one argument")]
#[case("print(\"a\".try_split(1))", "expected str")]
#[case("print((1).try_split(\",\"))", "expected str")]
#[case("print(\"a\".try_get())", "try_get requires one argument")]
#[case("print(\"a\".try_get(0, 1))", "try_get requires one argument")]
#[case("print(\"a\".try_get(0u8))", "expected i64")]
#[case("print(\"a\".try_get(0.0))", "expected i64")]
#[case("print((1).try_get(0))", "expected str")]
#[case(
    "mut text = \"abc\"\nloan = &mut text\nresult = text.try_get(0)\nprint(loan)",
    "borrow"
)]
#[case(
    "mut text = \"a:b\"\nloan = &mut text\nresult = text.try_split(\":\")\nprint(loan)",
    "borrow"
)]
#[case(
    "mut parts = [\"a\"]\nloan = &mut parts\nresult = \"-\".try_join(parts)\nloan.append(\"b\")",
    "borrow"
)]
fn rejects_invalid_text_building(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

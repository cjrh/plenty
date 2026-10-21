//! Length-aware UTF-8 building with recoverable allocation failure.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[test]
fn concat_and_join_preserve_utf8_nul_and_scalar_lengths() {
    native(
        r#"
def show() -> Result[(), AllocError]:
    combined = "é\0".concat("🙂")?
    parts = ["é", "", "🙂\0"].unwrap()
    joined = "界".join(parts)?
    print(len(combined)).unwrap()
    print(combined == "é\0🙂").unwrap()
    print(len(joined)).unwrap()
    print(joined == "é界界🙂\0").unwrap()
    print(len(parts)).unwrap()
    print("-".join([].unwrap())?).unwrap()
    print("-".join(["one"].unwrap())?).unwrap()
    print("".join(["a", "b"].unwrap())?).unwrap()
    print("".concat("")?).unwrap()
    Ok(())
print(str.repr(show()).unwrap()).unwrap()
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
    prefix.concat(", ".join(parts)?)?.concat("!")
def show(data: &Texts) -> Result[str, AllocError]:
    format(&data.prefix, &data.parts)
data = Texts("Hello ", ["Ada", "Bea"].unwrap())
print(str.repr(show(&data)).unwrap()).unwrap()
print(data.prefix).unwrap()
print(data.parts).unwrap()
"#,
        "Result[str, AllocError].Ok(\"Hello Ada, Bea!\")\nHello \n[\"Ada\", \"Bea\"]",
    );
}

#[test]
fn receiver_and_argument_are_evaluated_once_in_order() {
    native(
        r#"
def separator() -> str:
    print("separator").unwrap()
    "-"
def parts() -> list[str]:
    print("parts").unwrap()
    ["a", "b"].unwrap()
class Custom:
    def join(self, text: str) -> str:
        text
print(str.repr(separator().join(parts())).unwrap()).unwrap()
print(Custom().join("class method")).unwrap()
"#,
        "separator\nparts\nResult[str, AllocError].Ok(\"a-b\")\nclass method",
    );
}

#[test]
fn split_preserves_empty_fields_unicode_and_embedded_nuls() {
    native(
        r#"
def check() -> Result[(), AllocError]:
    pieces = "::é\0::🙂::::".split("::")?
    print(pieces).unwrap()
    print(len(pieces[1])).unwrap()
    print("::".join(pieces)? == "::é\0::🙂::::").unwrap()
    print("".split(",")?).unwrap()
    print("abc".split("absent")?).unwrap()
    print("aaaaa".split("aa")?).unwrap()
    print("a界b界".split("界")?).unwrap()
    print("a\0b\0".split("\0")?).unwrap()
    Ok(())
print(str.repr(check()).unwrap()).unwrap()
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
    text.split(separator)
def source() -> str:
    print("source").unwrap()
    "left:right"
def separator() -> str:
    print("separator").unwrap()
    ":"
class Custom:
    def split(self, text: str) -> str:
        text
text = Text("a:b")
sep = ":"
print(str.repr(split(&text.contents, &sep)).unwrap()).unwrap()
print(text.contents).unwrap()
print(str.repr(source().split(separator())).unwrap()).unwrap()
print(Custom().split("custom")).unwrap()
"#,
        "Result[list[str], AllocError].Ok([\"a\", \"b\"])\na:b\nsource\nseparator\nResult[list[str], AllocError].Ok([\"left\", \"right\"])\ncustom",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn splitting_recovers_at_every_allocation_and_preserves_sources() {
    // One entry buffer, one list owner, and three pieces too long to be inline.
    for budget in 0..=5 {
        native(
            &format!(
                r#"
source = ("alphabet:" + "betagamma:deltaepsilon").unwrap()
separator = ("" + ":").unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = source.split(separator)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(source).unwrap()
print(separator).unwrap()
print(str.repr(source.split(separator)).unwrap()).unwrap()
"#
            ),
            &format!(
                "Result[list[str], AllocError].{}\nalphabet:betagamma:deltaepsilon\n:\nResult[list[str], AllocError].Ok([\"alphabet\", \"betagamma\", \"deltaepsilon\"])",
                if budget < 5 {
                    "Err(AllocError.OutOfMemory)"
                } else {
                    "Ok([\"alphabet\", \"betagamma\", \"deltaepsilon\"])"
                }
            ),
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
        print("dropped").unwrap()
def split(source: str, guard: Guard) -> Result[list[str], AllocError]:
    pieces = source.split(":")?
    print("unreachable").unwrap()
    Ok(pieces)
source = ("alphabet:" + "betagamma:deltaepsilon").unwrap()
guard = Guard()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = split(source, guard)
print("__test_begin_no_allocations__").unwrap()
match result:
    case Ok(pieces):
        print("unexpected success").unwrap()
    case Err(error):
        print("handled").unwrap()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
"#
            ),
            "dropped\nhandled",
        );
    }
}

#[test]
fn split_rejects_empty_separator_at_runtime() {
    let output = support::run(
        "separator = \"\"\nprint(str.repr(\"abc\".split(separator)).unwrap()).unwrap()",
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("string split requires a nonempty separator")
    );
}

#[test]
fn checked_character_lookup_uses_scalar_indices_and_handles_extreme_bounds() {
    native(
        r#"
def show(text: &str, index: i64) -> Result[(), AllocError]:
    print(text.get(index)).unwrap()
    Ok(())
text = "é🙂\0"
for index in [-4, -3, -2, -1, 0, 1, 2, 3, -9223372036854775808, 9223372036854775807].unwrap():
    drop(show(&text, index))
empty = ""
drop(show(&empty, 0))
print(text == "é🙂\0").unwrap()
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
    print("source").unwrap()
    "é🙂"
def index() -> i64:
    print("index").unwrap()
    -1
class Custom:
    def get(self, index: i64) -> i64:
        index
text = Text("abc")
position = -2
print(text.value.get(&position)).unwrap()
print(text.value).unwrap()
print(source().get(index())).unwrap()
print(Custom().get(42)).unwrap()
"#,
        "Option[str].Some(\"b\")\nabc\nsource\nindex\nOption[str].Some(\"🙂\")\n42",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn character_lookup_never_allocates() {
    native(
        r#"
text = ("é" + "🙂").unwrap()
print("__test_begin_no_allocations__").unwrap()
missing = text.get(-3)
present = text.get(-1)
indexed = text[0]
print("__test_end_no_allocations__").unwrap()
print(missing).unwrap()
print(present).unwrap()
print(indexed).unwrap()
print(text).unwrap()
"#,
        "Option[str].Nothing\nOption[str].Some(\"🙂\")\né\né🙂",
    );
}

#[test]
fn index_expression_can_propagate_before_character_lookup() {
    native(
        r#"
def missing_index() -> Result[i64, AllocError]:
    print("index").unwrap()
    Err(AllocError.CapacityOverflow)
def lookup() -> Result[Option[str], AllocError]:
    Ok((("a" + "b").unwrap()).get(missing_index()?))
print(str.repr(lookup()).unwrap()).unwrap()
"#,
        "index\nResult[Option[str], AllocError].Err(AllocError.CapacityOverflow)",
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("left.concat(right)", "left-right")]
#[case("separator.join(parts)", "left|right")]
fn one_output_allocation_suffices_and_failure_can_be_retried(
    #[case] expression: &str,
    #[case] expected: &str,
) {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
left = ("le" + "ft").unwrap()
right = ("-ri" + "ght").unwrap()
separator = "|"
parts = [("le" + "ft").unwrap(), ("ri" + "ght").unwrap()].unwrap()
empty = list[str]().unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = {expression}
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(str.repr({expression}).unwrap()).unwrap()
print(left).unwrap()
print(right).unwrap()
print(parts).unwrap()
"#
            ),
            &format!(
                "Result[str, AllocError].{}\nResult[str, AllocError].Ok(\"{expected}\")\nleft\n-right\n[\"left\", \"right\"]",
                if budget == 0 {
                    "Err(AllocError.OutOfMemory)".into()
                } else {
                    format!("Ok(\"{expected}\")")
                }
            ),
        );
    }
}

#[test]
fn join_accepts_every_iterable_of_str() {
    native(
        r#"
def words(n: i64) -> Generator[str]:
    for i in range(n):
        yield "even" if i % 2 == 0 else "odd"
def show() -> Result[(), AllocError]:
    names = {"Ada"}.unwrap()
    scores = {"Ada": 1, "Bea": 2}.unwrap()
    print(", ".join(&names)?).unwrap()
    print(", ".join(scores)?).unwrap()
    print("-".join("hé🙂")?).unwrap()
    print(str.repr("-".join("")?).unwrap()).unwrap()
    print("-".join("z")?).unwrap()
    print("/".join(words(3))?).unwrap()
    print(str.repr("/".join(words(0))?).unwrap()).unwrap()
    later = words(2)
    print("+".join(later)?).unwrap()
    print("".join(w for w in ["a", "", "b"].unwrap() if w != "")?).unwrap()
    print(len(names)).unwrap()
    print(scores).unwrap()
    Ok(())
print(str.repr(show()).unwrap()).unwrap()
"#,
        "Ada\nAda, Bea\nh-é-🙂\n\"\"\nz\neven/odd/even\n\"\"\neven+odd\nab\n1\n{\"Ada\": 1, \"Bea\": 2}\nResult[(), AllocError].Ok(())",
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case("separator.join(&names)", "left|right")]
#[case("separator.join(\"left!\")", "l|e|f|t|!")]
fn measured_iterables_need_one_output_allocation(#[case] expression: &str, #[case] expected: &str) {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
separator = "|"
names = {{("le" + "ft").unwrap(): 1, ("ri" + "ght").unwrap(): 2}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = {expression}
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
print(names).unwrap()
"#
            ),
            &format!(
                "Result[str, AllocError].{}\n{{\"left\": 1, \"right\": 2}}",
                if budget == 0 {
                    "Err(AllocError.OutOfMemory)".into()
                } else {
                    format!("Ok(\"{expected}\")")
                }
            ),
        );
    }
}

/// Four 10-byte pieces and three 2-byte separators make 46 bytes. The scratch
/// buffer grows to 10, 20, 40, then 80 bytes; the result is the fifth
/// allocation. After each earlier failure, the generator and pieces are dropped
/// (the allocation-checks runtime checks the balance at exit).
#[cfg(feature = "runtime-checks")]
#[test]
fn generator_join_grows_scratch_geometrically_and_recovers_at_every_allocation() {
    let piece = "alpha-beta";
    let joined = [piece; 4].join(", ");
    for budget in 0..=5 {
        let expected = if budget < 5 {
            "Err(AllocError.OutOfMemory)".to_owned()
        } else {
            format!("Ok(\"{joined}\")")
        };
        native(
            &format!(
                r#"
def pieces() -> Generator[str]:
    for i in range(4):
        yield "{piece}"
generator = pieces()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = ", ".join(generator)
print("__test_restore_allocations__").unwrap()
print(str.repr(result).unwrap()).unwrap()
"#
            ),
            &format!("Result[str, AllocError].{expected}"),
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_join_releases_temporary_inputs_and_propagates_without_allocating() {
    native(
        r#"
class Guard:
    def __del__(self: &mut Guard) -> ():
        print("dropped").unwrap()
def build(parts: list[str], guard: Guard) -> Result[str, AllocError]:
    joined = "-".join(parts)?
    print("unreachable").unwrap()
    Ok(joined)
parts = [("ab" + "cd").unwrap(), ("ef" + "gh").unwrap()].unwrap()
guard = Guard()
print("__test_fail_allocations_after_0__").unwrap()
result = build(parts, guard)
print("__test_begin_no_allocations__").unwrap()
match result:
    case Ok(text):
        print("unexpected success").unwrap()
    case Err(error):
        print("handled").unwrap()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
"#,
        "dropped\nhandled",
    );
}

#[rstest]
#[case("print(\"a\".concat()).unwrap()", "concat requires one argument")]
#[case(
    "print(\"a\".join([].unwrap(), [].unwrap())).unwrap()",
    "join requires one argument"
)]
#[case("print(\"a\".concat(1)).unwrap()", "expected str")]
#[case(
    "print(\"a\".join([1, 2].unwrap())).unwrap()",
    "expected list[str], got list[i64]"
)]
#[case(
    "xs = [1, 2].unwrap()\nprint(\"a\".join(&xs)).unwrap()",
    "join requires an iterable of str (a list, set, dictionary keys, str, or generator), got list[i64]"
)]
#[case(
    "print(\"a\".join(range(3))).unwrap()",
    "join requires an iterable of str (a list, set, dictionary keys, str, or generator), got range"
)]
#[case(
    "d = {1: \"x\"}.unwrap()\nprint(\"a\".join(&d)).unwrap()",
    "join requires an iterable of str (a list, set, dictionary keys, str, or generator), got dict[i64, str]"
)]
#[case(
    "def words() -> Generator[str]:\n    yield \"x\"\ng = words()\nprint(\"a\".join(&g)).unwrap()",
    "join consumes a generator; pass it by value, not by reference"
)]
#[case(
    "def words() -> Generator[str]:\n    yield \"x\"\nmut g = words()\nprint(\"a\".join(&mut g)).unwrap()",
    "join consumes a generator; pass it by value, not by reference"
)]
#[case(
    "def words() -> Generator[str]:\n    yield \"x\"\ndef show(g: &mut Generator[str]) -> Result[str, AllocError]:\n    \"a\".join(g)\nmut g = words()\nprint(show(&mut g).unwrap()).unwrap()",
    "join consumes a generator; pass it by value, not by reference"
)]
#[case(
    "def words() -> Generator[str]:\n    yield \"x\"\ng = words()\nfirst = \"a\".join(g)\nsecond = \"a\".join(g)",
    "use of moved binding `g`"
)]
#[case("print((1).concat(\"a\")).unwrap()", "expected str")]
#[case(
    "print(\"a\".split()).unwrap()",
    "print cannot print a Result directly"
)]
#[case("print(\"a\".split(\",\", 1)).unwrap()", "split requires one argument")]
#[case("print(\"a\".split(1)).unwrap()", "expected str")]
#[case("print((1).split(\",\")).unwrap()", "expected str")]
#[case("print(\"a\".get()).unwrap()", "get requires one argument")]
#[case("print(\"a\".get(0, 1)).unwrap()", "get requires one argument")]
#[case("print(\"a\".get(0u8)).unwrap()", "expected i64")]
#[case("print(\"a\".get(0.0)).unwrap()", "expected i64")]
#[case("print((1).get(0)).unwrap()", "unsupported method")]
#[case(
    "mut text = \"abc\"\nloan = &mut text\nresult = text.get(0)\nprint(loan).unwrap()",
    "borrow"
)]
#[case(
    "mut text = \"a:b\"\nloan = &mut text\nresult = text.split(\":\")\nprint(loan).unwrap()",
    "borrow"
)]
#[case(
    "mut parts = [\"a\"].unwrap()\nloan = &mut parts\nresult = \"-\".join(parts)\nloan.append(\"b\").unwrap()",
    "borrow"
)]
fn rejects_invalid_text_building(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

//! Borrowed substrings `&s[a:b]`, of type `&strview` (issue #595). See
//! book/src/design/language/04-expressions/slices.md#substring-views.
mod support;

use rstest::rstest;

/// Bounds count Unicode scalars and clamp as `s[a:b]?` does, for inline
/// (seven bytes or fewer) and heap strings alike.
#[test]
fn bounds_count_scalars_and_clamp() {
    support::assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    s = "héllo, wörld"
    t = "ab"
    print(&s[1:5], &s[-5:], &s[:2], len(&s[1:5]), (&s[1:5]).byte_len())?
    print(len(&s[9:2]), len(&s[-100:100]), len(&s[40:]), &t[-1:], len(&t[:]))?
    v = &s[7:]
    w = &v[1:3]
    print(w, v[0], v[-1])?
    print(s[1:5]? == &s[1:5], v[1:3]?)?
    Ok(())
"#,
        "éllo wörld hé 4 5\n0 12 0 b 2\nör w d\nTrue ör\n",
    );
}

#[test]
fn iteration_yields_one_scalar_strings() {
    support::assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    s = "naïve café"
    for c in &s[:]:
        if c == " ":
            break
        if c == "ï":
            continue
        print(c)?
    for c in reversed(&s[6:]):
        print(c)?
    for i, c in enumerate(&s[2:4]):
        print(i, c)?
    print(iter(&s[:5]).count())?
    Ok(())
"#,
        "n\na\nv\ne\né\nf\na\nc\n0 ï\n1 v\n5\n",
    );
}

/// Comparison is by bytes, which for UTF-8 is scalar order, as for `str`.
#[test]
fn views_compare_with_views_and_strings() {
    support::assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    s = "apple banana"
    v = &s[:5]
    print(v == "apple", "apple" == v, v != "apple", v == &s[6:])?
    print(v < "b", &s[6:] > v, v <= "apple", v >= "apples", &s[:0] == "")?
    print("pl" in v, "na" in v, "na" not in v, &s[1:3] in &s[:])?
    print(v.startswith("ap"), v.endswith(&s[3:5]), v.find("l"), v.find("z"))?
    Ok(())
"#,
        "True True False False\nTrue True True False True\nTrue False True True\nTrue True Option[i64].Some(3) Option[i64].Nothing\n",
    );
}

#[test]
fn views_format_as_their_text() {
    support::assert_stdout(
        r#"
import std.fmt

def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:
    s = "say \"héllo\" now"
    v = &s[4:11]
    print(v)?
    print(str.repr(v)?)?
    print(f"[{v}] [{v!r}] [{v:>9}] [{v:.3}] [{v!a}]"?)?
    Ok(())
"#,
        "\"héllo\"\n\"\\\"héllo\\\"\"\n[\"héllo\"] [\"\\\"héllo\\\"\"] [  \"héllo\"] [\"hé] [\"\\\"h\\u{e9}llo\\\"\"]\n",
    );
}

#[test]
fn substr_parameters_take_part_of_a_string() {
    support::assert_stdout(
        r#"
def count(text: &strview, scalar: &str) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    for c in text:
        if c == scalar:
            n += 1
    Ok(n)

def words(text: &str) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    mut start = 0
    for i, c in enumerate(text):
        if c == " ":
            if count(&text[start:i], "o")? > 0:
                n += 1
            start = i + 1
    Ok(n)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    s = "one two three four"
    print(count(&s[:], "o")?, count(&s[4:], "o")?, words(&s)?)?
    Ok(())
"#,
        "3 2 2\n",
    );
}

/// A borrowed `str`, a `&str` binding, or a literal converts to the whole
/// `&strview` (consistency rule 3, backlog B90).
#[test]
fn strings_convert_to_whole_substr_views() {
    support::assert_stdout(
        r#"
def vowels(text: &strview) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    for c in text:
        if c in "aeiou":
            n += 1
    Ok(n)

def through(text: &str) -> Result[i64, OverflowError[i64]]:
    vowels(text)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    s = "education"
    shared = Shared("queue")?
    print(vowels(&s)?, vowels(&s[:3])?, vowels("aei")?, through(&s)?, vowels(&shared)?)?
    whole: &strview = &s
    literal: &strview = "hello"
    print(whole, len(literal))?
    Ok(())
"#,
        "5 2 3 5 4\neducation 5\n",
    );
}

#[test]
fn substr_calls_implicitly_borrow_string_places_and_forward_references() {
    support::assert_stdout(
        r#"
class Label:
    text: str

    def width(self, text: &strview) -> i64:
        len(text)

def width(text: &strview) -> i64:
    len(text)

def generic[T](text: &strview, other: T) -> i64:
    len(text)

def reference(text: &str) -> &str:
    text

def index() -> i64:
    print("index").unwrap()
    0

def main() -> Result[(), AllocError | IoError]:
    mut s = "é🙂"
    heap = ("héllo, " + "wörld")?
    label = Label(heap)
    values = [heap]?
    r = &heap
    print(width(s), width(heap), width(r), width(reference(heap)))?
    print(width(label.text), width(values[index()]), width(heap[7:]?))?
    callable = width
    closure = def(text: &strview) -> i64:
        len(text)
    print(width(text=s), generic(s, 1), label.width(s), callable(s), closure(s))?
    s = "new"
    print(s, heap)?
    Ok(())
"#,
        "2 12 12 12\nindex\n12 12 5\n2 2 2 2 2\nnew héllo, wörld\n",
    );
}

#[test]
fn implicit_substr_temporaries_are_evaluated_once_and_cleaned_on_later_errors() {
    support::assert_stdout(
        r#"
def make() -> str:
    print("make").unwrap()
    ("héllo" + " world").unwrap()

def later() -> i64:
    print("later").unwrap()
    9

def fail() -> Result[i64, str]:
    print("fail").unwrap()
    Err("stop")

def width(text: &strview, extra: i64) -> i64:
    print(text, extra).unwrap()
    len(text)

def attempt() -> Result[i64, str]:
    Ok(width(make(), fail()?))

def tail() -> i64:
    width(make(), 9)

def main() -> Result[(), IoError]:
    print(width(make(), later()))?
    print(attempt().is_err())?
    print(tail())?
    Ok(())
"#,
        "make\nlater\nhéllo world 9\n11\nmake\nfail\nTrue\nmake\nhéllo world 9\n11\n",
    );
}

#[test]
fn view_index_is_checked_against_the_view() {
    let output = support::run(
        r#"
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    s = "héllo"
    v = &s[1:3]
    i = len(s) - 3
    print(v[i])?
    Ok(())
"#,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("index out of bounds: index 2, length 2"),
        "{stderr}"
    );
}

#[rstest]
#[case("s = \"abc\"\nv = &mut s[1:]", "write `&s[a:b]`")]
#[case(
    "def f(x: strview) -> ():\n    pass",
    "`strview` is only the borrowed form `&strview`"
)]
#[case("def f(x: &mut strview) -> ():\n    pass", "write `&strview`")]
#[case("s = \"abc\"\nv = &s[::2]", "a slice view selects adjacent elements")]
#[case(
    "s = \"abc\"\nv = &s[:]\nr = &v[0]",
    "a `&strview` has no element slots to borrow"
)]
#[case(
    "mut s = \"abc\"\nv = &s[1:]\ns = \"zz\"\nn = len(v)",
    "cannot assign to `s` while it is borrowed"
)]
#[case(
    "s = \"abc\"\nxs = [&s[1:]].unwrap()",
    "slice views cannot be stored in collections yet"
)]
#[case(
    "def g(x: &strview, owner: &mut str) -> ():\n    pass\n\nmut s = \"abc\"\ng(s, &mut s)",
    "conflicting borrow"
)]
#[case(
    "def g(x: &strview, owner: &mut list[str]) -> ():\n    pass\n\nmut xs = [\"abc\"].unwrap()\ng(xs[0], &mut xs)",
    "conflicting borrow"
)]
#[case(
    "def g(x: &strview) -> i64:\n    len(x)\n\nn = g(b\"abc\")",
    "this parameter takes a substring view"
)]
#[case(
    "def g[T](xs: &[T]) -> i64:\n    len(xs)\n\ns = \"abc\"\nn = g(&s)",
    "`&s[a:b]` is a `&strview`"
)]
#[case(
    "def g[T](xs: &[T]) -> i64:\n    len(xs)\n\ns = \"abc\"\nn = g(&s[:])",
    "`&s[a:b]` is a `&strview`"
)]
fn invalid_substr_views_are_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_diagnostic(source);
    assert!(error.contains(expected), "{source}\n{error}");
}

/// Owner-reference parameters for the view mismatch cases below.
const OWNER_PARAMETERS: &str = r#"
from std.bytes import Bytes, from_text

def f(s: &str) -> i64:
    s.byte_len()

class Holder:
    n: i64

    def take(self: &Holder, s: &str) -> i64:
        s.byte_len()

def total(xs: &list[i64]) -> i64:
    xs[0]

def bump(xs: &mut list[i64]) -> ():
    xs[0] = 5

def raw(b: &bytes) -> i64:
    len(b)

def buffer(b: &Bytes) -> i64:
    b.len()

def main() -> Result[(), AllocError | IoError]:
    s = "hello"
"#;

/// A view at a `&str` or `&list[T]` parameter names the mismatch and both
/// remedies, instead of asking for a `?` that the view rejects (issue #802).
#[rstest]
#[case(
    "n = f(&s[1:])",
    "a &strview view, but the parameter takes &str; declare the parameter `&strview` to borrow it without copying, or pass an owned copy, `s[a:b]?`"
)]
#[case("n = f(s=&s[1:3])", "a &strview view, but the parameter takes &str")]
#[case("i = 1\nn = f(&s[i:len(s)])", "or pass an owned copy, `s[a:b]?`")]
#[case("n = f(&(s[:2]))", "declare the parameter `&strview`")]
#[case("n = f(&s[1:]?)", "copy the elements with `xs[a:b]?`")]
#[case("n = f(&\"abc\"[1:])", "a &strview view")]
#[case("v = &s[1:]\nn = f(v)", "or pass an owned copy, `v[:]?`")]
#[case("v = &s[1:]\nn = f(&v)", "or pass an owned copy, `v[:]?`")]
#[case(
    "h = Holder(n=1)\nn = h.take(&s[1:])",
    "a &strview view, but the parameter takes &str"
)]
#[case(
    "xs = [1, 2].unwrap()\nn = total(&xs[1:])",
    "a &[i64] view, but the parameter takes &list[i64]; declare the parameter `&[i64]` to borrow it without copying, or pass an owned copy, `xs[a:b]?`"
)]
#[case(
    "xs = [1, 2].unwrap()\nv = &xs[1:]\nn = total(v)",
    "`xs[a:b]?` from the list or buffer the view borrows"
)]
#[case(
    "xs = [1, 2, 3].unwrap()\nv = &xs[:]\nn = total(&v[1:])",
    "`xs[a:b]?` from the list or buffer the view borrows"
)]
#[case(
    "b = b\"abc\"\nn = raw(&b[1:])",
    "a &[u8] view, but the parameter takes &bytes; declare the parameter `&[u8]` to borrow it without copying"
)]
#[case(
    "buf = from_text(\"abc\").unwrap()\nn = buffer(&buf[1:])",
    "declare the parameter `&[u8]` to borrow it without copying, or pass an owned copy, `xs[a:b]?`"
)]
#[case("xs = [1, 2].unwrap()\nn = f(&xs[1:])", "expected &str, got &[i64]")]
#[case("n = total(&s[1:])", "expected &list[i64], got &strview")]
#[case("b = b\"abc\"\nn = f(&b[1:])", "expected &str, got &[u8]")]
#[case(
    "n = f(&s[::2])",
    "a slice view selects adjacent elements, so it takes no step"
)]
#[case(
    "xs = [1, 2].unwrap()\nn = total(&xs[::2])",
    "a slice view selects adjacent elements, so it takes no step"
)]
#[case(
    "mut xs = [1, 2].unwrap()\nbump(&mut xs[1:])",
    "a &mut [i64] view, but the parameter takes &mut list[i64]; declare the parameter `&mut [i64]` to change the elements in place"
)]
fn views_at_owner_reference_parameters_name_the_mismatch(
    #[case] call: &str,
    #[case] expected: &str,
) {
    let source = format!(
        "{OWNER_PARAMETERS}    {}\n    Ok(())\n",
        call.replace('\n', "\n    ")
    );
    let error = support::check_file_error(&source);
    assert!(error.contains(expected), "{source}\n{error}");
    assert!(!error.contains("use `?`"), "{source}\n{error}");
    if call.contains("raw(") {
        assert!(!error.contains("owned copy"), "{source}\n{error}");
    }
}

/// Both remedies the mismatch diagnostic names compile (issue #802).
#[test]
fn view_mismatch_remedies_compile() {
    support::assert_stdout(
        r#"
from std.bytes import Bytes, from_text

def borrowed(s: &strview) -> i64:
    s.byte_len()

def owned(s: &str) -> i64:
    s.byte_len()

def listed(xs: &[i64]) -> i64:
    len(xs)

def whole(xs: &list[i64]) -> i64:
    len(xs)

def buffer(b: &Bytes) -> i64:
    b.len()

def main() -> Result[(), AllocError | IoError]:
    s = "hello"
    v = &s[1:]
    print(borrowed(&s[1:]), borrowed(s=&s[1:3]), borrowed(v), borrowed(&s))?
    print(owned(s[1:]?), owned(s=s[1:3]?), owned(v[:]?), owned(v[1:]?))?
    xs = [1, 2, 3]?
    rows = &xs[:]
    print(listed(&rows[1:]), whole(xs[1:]?))?
    buf = from_text("abc")?
    print(buffer(buf[1:]?))?
    Ok(())
"#,
        "4 2 4 5\n4 2 4 3\n2 2\n2\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn views_add_no_allocations() {
    let output = support::run(
        r#"
def count(text: &strview, scalar: &str) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    for c in text:
        if c == scalar:
            n += 1
    Ok(n)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    s = ("a longer heap string, with " + "ünïcode")?
    mut sum = 0
    print("__test_begin_no_allocations__")?
    v = &s[2:8]
    sum += count(s, "n")? + len(v) + v.byte_len()
    for c in reversed(&v[1:]):
        if c == "g":
            sum += 100
    if v == "longer" and v > &s[9:] and "ng" in v and v.startswith("lo"):
        sum += 1000
    sum += v.find("e").unwrap_or(-1)
    print("__test_end_no_allocations__")?
    print(sum)?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).ends_with("1119\n"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn as_bytes_borrows_exact_utf8_ranges_and_preserves_scalar_iteration() {
    support::assert_stdout(
        r#"
import std.bytes

def show(text: &str) -> Result[(), IoError]:
    print(text.as_bytes())

def show_view(text: &strview) -> Result[(), IoError]:
    print(text.as_bytes())

def main() -> Result[(), AllocError | IoError]:
    text = "aé\0🙂z"
    bytes: &[u8] = text.as_bytes()
    print(len(bytes), len(text), bytes[1], bytes[-1])?
    print((&text[1:4]).as_bytes(), &text.as_bytes()[1:4])?
    print(std.bytes.from_text(&text[1:4])?)?
    print("é".as_bytes(), "".as_bytes())?
    show(&text)?
    show_view(&text[1:2])?
    for byte in (&text[1:2]).as_bytes():
        print(*byte)?
    for scalar in &text[1:2]:
        print(scalar)?
    Ok(())
"#,
        "9 5 195 122\nb\"\\xc3\\xa9\\x00\\xf0\\x9f\\x99\\x82\" b\"\\xc3\\xa9\\x00\"\nb\"\\xc3\\xa9\\x00\\xf0\\x9f\\x99\\x82\"\nb\"\\xc3\\xa9\" b\"\"\nb\"a\\xc3\\xa9\\x00\\xf0\\x9f\\x99\\x82z\"\nb\"\\xc3\\xa9\"\n195\n169\né\n",
    );
}

#[test]
fn as_bytes_tracks_inline_field_collection_and_shared_origins() {
    support::assert_stdout(
        r#"
import std.bytes

class Label:
    text: str

def replace_after_use(text: &mut str) -> Result[(), IoError]:
    bytes = text.as_bytes()
    print(bytes)?
    *text = "changed"
    Ok(())

def read[T: CopyType](value: &T) -> T:
    *value

def main() -> Result[(), AllocError | IoError]:
    mut label = Label("é")
    field = label.text.as_bytes()
    print(field)?
    label.text = "x"
    mut values = ["abc", "é"]?
    element = values[1].as_bytes()
    print(element)?
    values.append("after")?
    entries = {"key": "abc"}?
    print(entries["key"].as_bytes())?
    shared = Shared("é")?
    print(shared.as_bytes())?
    mut small = "abc"
    replace_after_use(&mut small)?
    print(small, read(&small))?
    mut output = std.bytes.with_capacity(8)?
    output.extend((&values[0][1:]).as_bytes())?
    print(output)?
    Ok(())
"#,
        "b\"\\xc3\\xa9\"\nb\"\\xc3\\xa9\"\nb\"abc\"\nb\"\\xc3\\xa9\"\nb\"abc\"\nchanged changed\nb\"bc\"\n",
    );
}

#[test]
fn as_bytes_temporary_owner_survives_consuming_call() {
    support::assert_stdout(
        r#"
import std.bytes

def make() -> str:
    return "heap-backed temporary string"

def width(bytes: &[u8]) -> i64:
    len(bytes)

def tiny() -> str:
    return "é!"

def main() -> Result[(), AllocError | IoError]:
    print(width(make().as_bytes()), std.bytes.from_bytes(make().as_bytes())?)?
    print(width((&make()[12:]).as_bytes()))?
    for byte in tiny().as_bytes():
        print(*byte)?
    Ok(())
"#,
        "28 b\"heap-backed temporary string\"\n16\n195\n169\n33\n",
    );
}

#[rstest]
#[case(
    "mut text = \"abc\"\n    bytes = text.as_bytes()\n    text = \"new\"\n    print(bytes)?",
    "borrow"
)]
#[case(
    "mut text = \"abc\"\n    bytes = text.as_bytes()\n    reference = &mut text\n    print(bytes)?",
    "borrow"
)]
#[case(
    "mut values = [\"abc\"]?\n    bytes = values[0].as_bytes()\n    values.append(\"new\")?\n    print(bytes)?",
    "borrow"
)]
#[case(
    "text = \"abc\"\n    mut bytes = text.as_bytes()\n    bytes[0] = 0u8",
    "shared"
)]
fn as_bytes_keeps_storage_immutable_until_last_use(#[case] body: &str, #[case] message: &str) {
    let program =
        format!("def main() -> Result[(), AllocError | IoError]:\n    {body}\n    Ok(())\n");
    let error = support::check_file_error(&program);
    assert!(error.contains(message), "{error}");
}

#[rstest]
#[case("make().as_bytes()")]
#[case("&make().as_bytes()")]
#[case("&make().as_bytes()[:]")]
#[case("(&make()[:]).as_bytes()")]
fn as_bytes_cannot_save_a_temporary_owner_view(#[case] expression: &str) {
    let program = format!(
        "def make() -> str:\n    return \"abc\"\n\ndef main() -> Result[(), IoError]:\n    bytes = {expression}\n    print(bytes)?\n    Ok(())\n"
    );
    let error = support::check_file_error(&program);
    assert!(error.contains("temporary string"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn as_bytes_allocates_nothing_and_direct_view_copy_reports_failure() {
    support::assert_stdout(
        r#"
import std.bytes

def main() -> Result[(), AllocError | IoError]:
    text = "é: long heap-backed text"
    view = &text[1:]
    print("__test_fail_allocations_after_0__")?
    bytes = view.as_bytes()
    count = len(bytes)
    first = bytes[0]
    literal = "é".as_bytes()
    copied = std.bytes.from_text(view)
    print("__test_restore_allocations__")?
    print(count, first, literal)?
    print(copied.is_err(), std.bytes.from_text(view)?)?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_restore_allocations__\n23 58 b\"\\xc3\\xa9\"\nTrue b\": long heap-backed text\"\n",
    );
}

#[rstest]
#[case("view = &text[:] if True else &other[:]")]
#[case("view: &strview = &text[:] if True else &other[:]")]
#[case("print(&text[:] if True else &other[:])?")]
#[case("bytes = text.as_bytes() if True else other.as_bytes()")]
#[case("bytes: &[u8] = text.as_bytes() if True else other.as_bytes()")]
#[case("print(text.as_bytes() if True else other.as_bytes())?")]
fn conditional_byte_views_are_rejected_without_losing_their_origins(#[case] statement: &str) {
    let program = format!(
        "def main() -> Result[(), IoError]:\n    text = \"abc\"\n    other = \"def\"\n    {statement}\n    Ok(())\n"
    );
    let error = support::check_file_error(&program);
    assert!(
        error.contains("conditional expressions cannot produce slice views"),
        "{error}"
    );
}

/// The canonical view name and original alias resolve to the same view type;
/// ordinary references to owning handles still follow generic reference rules.
#[test]
fn strview_name_preserves_aliases_and_ordinary_references() {
    support::assert_stdout(
        r#"
type TextView = &strview

def canonical(text: &strview) -> i64:
    len(text)

def legacy(text: &substr) -> i64:
    canonical(text)

def owner[T: CopyType](value: &T) -> T:
    *value

def first[T](values: &list[T]) -> &T:
    &values[0]

def main() -> Result[(), AllocError | IoError]:
    text = "héllo, wörld"
    ordinary: &str = &text
    whole: &strview = &text
    alias: &substr = whole
    narrowed: TextView = &alias[7:]
    canonical_again: &strview = alias
    print(canonical(text), canonical(ordinary), legacy(whole), legacy(narrowed))?
    print(narrowed, canonical_again == text, owner(ordinary))?
    values = [text]?
    returned: &str = first(&values)
    retained: str = *returned
    print(retained)?
    Ok(())
"#,
        "12 12 12 5\nwörld True héllo, wörld\nhéllo, wörld\n",
    );
}

#[rstest]
#[case(
    "def invalid(text: strview) -> ():\n    pass",
    "`strview` is only the borrowed form `&strview`"
)]
#[case(
    "def invalid(text: substr) -> ():\n    pass",
    "`&substr` is a compatibility alias"
)]
#[case("def invalid(text: &mut strview) -> ():\n    pass", "write `&strview`")]
#[case("def invalid(text: &mut substr) -> ():\n    pass", "write `&strview`")]
#[case("text = \"abc\"\nview: &strview = text", "expected &strview, got str")]
#[case("text = \"abc\"\nview: &substr = text", "expected &strview, got str")]
fn strview_type_rules_and_alias_diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_diagnostic(source);
    assert!(error.contains(expected), "{source}\n{error}");
}

#[test]
fn text_view_builtin_parity_and_mixed_operands() {
    support::assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    text = "!hé hé!"
    v = &text[1:-1]
    needle = &text[1:3]
    print(v.get(0), v.get(-1), v.get(99), v.rfind(needle), v.count(needle))?
    print(v.startswith(needle), v.endswith(needle), needle in text, needle not in text)?
    print(text.find(needle), text.rfind(needle), text.count(needle))?
    print(v.lower()?, v.upper()?, v.concat(needle)?, (v + "!")?, ("!" + v)?)?
    print(v.replace(needle, "a")?, text.replace("!", needle)?)?
    print(v.removeprefix(needle)?, v.removesuffix(needle)?, v.repeat(2)?)?
    print(v.slice(1, 4)?, v[::-1]?, v[::2]?, v[-100:100:2]?)?
    print(v.split()?, v.split(" ")?, text.split(needle)?)?
    pad = "  Hé\t"
    p = &pad[:]
    print(p.strip()?, p.lstrip()?, p.rstrip()?, p.isascii(), p.isalpha())?
    digits = "123"
    d = &digits[:]
    print(d.isdigit(), d.isalnum(), d.isascii(), d.isspace(), (&pad[:2]).isspace())?
    line = "a\r\nb\nc"
    l = &line[:]
    print(l.splitlines()?, l.splitlines(True)?)?
    Ok(())
"#,
        concat!(
            "Option[str].Some(\"h\") Option[str].Some(\"é\") Option[str].Nothing Option[i64].Some(3) 2\n",
            "True True True False\nOption[i64].Some(1) Option[i64].Some(4) 2\n",
            "hé hé HÉ HÉ hé héhé hé hé! !hé hé\na a héhé héhé\n hé hé  hé héhé hé\n",
            "é h éh éh h é h é\n",
            "[\"hé\", \"hé\"] [\"hé\", \"hé\"] [\"!\", \" \", \"!\"]\n",
            "Hé Hé\t   Hé False False\nTrue True True False True\n",
            "[\"a\", \"b\", \"c\"] [\"a\\r\\n\", \"b\\n\", \"c\"]\n",
        ),
    );
}

#[test]
fn text_view_join_supports_existing_iterables_and_temporary_strings() {
    support::assert_stdout(
        r#"
def source() -> str:
    print("source").unwrap()
    "hé🙂"

def main() -> Result[(), AllocError | IoError]:
    separator = "|-|"
    sep = &separator[1:2]
    letters = "hé🙂"
    text = &letters[:]
    pieces = ["hé", "🙂"]?
    keys = {"b": 2, "a": 1}?
    print(sep.join("ab")?, sep.join(source())?, "-".join(text)?, sep.join(text)?)?
    print(sep.join(pieces)?, sep.join(&pieces[:])?, sep.join(keys)?)?
    generator = (word for word in ["a", "b"]?)
    print(sep.join(generator)?, sep.join([]?)?, sep.join("")?)?
    Ok(())
"#,
        "source\na-b h-é-🙂 h-é-🙂 h-é-🙂\nhé-🙂 hé-🙂 b-a\na-b  \n",
    );
}

#[test]
fn text_view_numeric_parse_and_ord_preserve_errors() {
    support::assert_stdout(
        r#"
def main() -> Result[(), IoError]:
    digits = "! 255 !"
    value = &digits[1:-1]
    print(str.repr(u8.parse(value)).unwrap(), str.repr(i8.parse(value)).unwrap(), str.repr(f64.parse(value)).unwrap())?
    scalar = "!é!"
    print(str.repr(ord(&scalar[1:2])).unwrap(), str.repr(ord(&scalar[:0])).unwrap(), str.repr(ord(&scalar[:])).unwrap())?
    invalid = "12x"
    print(str.repr(i64.parse(&invalid[:])).unwrap())?
    Ok(())
"#,
        "Result[u8, ParseError].Ok(255) Result[i8, ParseError].Err(ParseError.OutOfRange) Result[f64, ParseError].Ok(255.0)\nResult[u32, ParseError].Ok(233) Result[u32, ParseError].Err(ParseError.Invalid) Result[u32, ParseError].Err(ParseError.Invalid)\nResult[i64, ParseError].Err(ParseError.Invalid)\n",
    );
}

#[test]
fn text_view_method_chains_keep_owned_result_hints() {
    support::assert_stdout(
        r#"
def main() -> Result[(), AllocError | IoError]:
    source = "!HÉ!"
    v = &source[1:-1]
    needle_source = "é"
    needle = &needle_source[:]
    print(v.lower()?.concat(needle)?, v.lower()?.find(needle))?
    print((v + "!")?.replace(needle, "a")?, "-".join(v.lower()?)?)?
    Ok(())
"#,
        "héé Option[i64].Some(1)\nHÉ! h-é\n",
    );
}

#[test]
fn range_operations_hold_source_loans_through_all_arguments() {
    for source in [
        "mut s = \"héllo\"\nv = &s[:]\ndef change(x: &mut str) -> str:\n    *x = \"new\"\n    \"x\"\nn = v.replace(\"h\", change(&mut s))",
        "mut s = \"héllo\"\nv = &s[:]\ndef change(x: &mut str) -> str:\n    *x = \"new\"\n    \"x\"\nn = v.join(change(&mut s))",
    ] {
        let error = support::check_diagnostic(source);
        assert!(error.contains("borrow"), "{error}");
    }
    support::assert_stdout(
        r#"
class Label:
    text: str

def main() -> Result[(), AllocError | IoError]:
    mut labels = [Label("HÉ"), Label("🙂")]?
    v = &labels[0].text[:]
    result = v.lower()?.concat(&labels[1].text[:])?
    labels = []?
    print(result)?
    Ok(())
"#,
        "hé🙂\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn range_readers_do_not_allocate_and_owned_results_fail_cleanly() {
    support::assert_stdout(
        r#"
def main() -> Result[(), IoError]:
    text = "!long héllo text!"
    v = &text[1:-1]
    digits = "123"
    d = &digits[:]
    print("__test_fail_allocations_after_0__")?
    found = v.find("hé")
    number = i64.parse(d)
    scalar = ord(&text[6:7])
    upper_result = v.upper()
    repeated = v.repeat(2)
    pieces = v.split()
    copy = v[:]
    print("__test_restore_allocations__")?
    print(found, str.repr(number).unwrap(), str.repr(scalar).unwrap())?
    print(upper_result.is_err(), repeated.is_err(), pieces.is_err(), copy.is_err())?
    Ok(())
"#,
        "__test_fail_allocations_after_0__\n__test_restore_allocations__\nOption[i64].Some(5) Result[i64, ParseError].Ok(123) Result[u32, ParseError].Ok(104)\nTrue True True True\n",
    );
}

#[test]
fn text_stream_writers_borrow_selected_ranges_and_count_scalars() {
    let output = support::run(
        r#"
def main() -> Result[(), IoError]:
    text = "!hé🙂!"
    selected = &text[1:-1]
    count = write_stdout(selected)?
    errors = write_stderr(selected)?
    print(count, errors)?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hé🙂3 3\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "hé🙂");
}

#[test]
fn scalar_index_results_are_implicitly_borrowed_as_owned_text() {
    let output = support::run(
        r#"
def scalar(text: &strview) -> Result[u32, ParseError]:
    return ord(text)

def ordinary(text: &str) -> Result[u32, ParseError]:
    return ord(text)

def main() -> Result[(), IoError | ParseError]:
    text = "!hé🙂!"
    selected = &text[1:-1]
    print(scalar(selected[0])?, scalar(selected[1])?, ordinary(selected[2])?)?
    print(selected[1].byte_len(), selected[2].as_bytes()[0])?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "104 233 128578\n2 240\n"
    );
}

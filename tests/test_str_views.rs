//! Borrowed substrings `&s[a:b]`, of type `&substr` (issue #595). See
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
def count(text: &substr, scalar: &str) -> Result[i64, OverflowError[i64]]:
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
/// `&substr` (consistency rule 3, backlog B90).
#[test]
fn strings_convert_to_whole_substr_views() {
    support::assert_stdout(
        r#"
def vowels(text: &substr) -> Result[i64, OverflowError[i64]]:
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
    whole: &substr = &s
    literal: &substr = "hello"
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

    def width(self, text: &substr) -> i64:
        len(text)

def width(text: &substr) -> i64:
    len(text)

def generic[T](text: &substr, other: T) -> i64:
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
    closure = def(text: &substr) -> i64:
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

def width(text: &substr, extra: i64) -> i64:
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
    "def f(x: substr) -> ():\n    pass",
    "`substr` is only the borrowed form `&substr`"
)]
#[case("def f(x: &mut substr) -> ():\n    pass", "write `&substr`")]
#[case("s = \"abc\"\nv = &s[::2]", "a slice view selects adjacent elements")]
#[case(
    "s = \"abc\"\nv = &s[:]\nr = &v[0]",
    "a `&substr` has no element slots to borrow"
)]
#[case(
    "s = \"abc\"\nv = &s[:1]\nb = v in s",
    "search for a `&substr` with `&s[:]`"
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
    "def f(s: &str) -> &substr:\n    &s[1:]",
    "a function cannot return a slice view yet (backlog B11)"
)]
#[case(
    "def g(x: &substr, owner: &mut str) -> ():\n    pass\n\nmut s = \"abc\"\ng(s, &mut s)",
    "conflicting borrow"
)]
#[case(
    "def g(x: &substr, owner: &mut list[str]) -> ():\n    pass\n\nmut xs = [\"abc\"].unwrap()\ng(xs[0], &mut xs)",
    "conflicting borrow"
)]
#[case(
    "def g(x: &substr) -> i64:\n    len(x)\n\nn = g(b\"abc\")",
    "this parameter takes a substring view"
)]
#[case(
    "def g[T](xs: &[T]) -> i64:\n    len(xs)\n\ns = \"abc\"\nn = g(&s)",
    "`&s[a:b]` is a `&substr`"
)]
#[case(
    "def g[T](xs: &[T]) -> i64:\n    len(xs)\n\ns = \"abc\"\nn = g(&s[:])",
    "`&s[a:b]` is a `&substr`"
)]
#[case(
    "s = \"abc\"\nv = &s[:]\nn = v.upper()",
    "a `&substr` has no method `upper`"
)]
fn invalid_substr_views_are_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_diagnostic(source);
    assert!(error.contains(expected), "{source}\n{error}");
}

/// Owner-reference parameters for the view mismatch cases below.
const OWNER_PARAMETERS: &str = r#"
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

s = "hello"
"#;

/// A view at a `&str` or `&list[T]` parameter names the mismatch and both
/// remedies, instead of asking for a `?` that the view rejects (issue #802).
#[rstest]
#[case(
    "n = f(&s[1:])",
    "a &substr view, but the parameter takes &str; declare the parameter `&substr` to borrow it without copying, or pass an owned copy, `s[a:b]?`"
)]
#[case("n = f(s=&s[1:3])", "a &substr view, but the parameter takes &str")]
#[case("i = 1\nn = f(&s[i:len(s)])", "or pass an owned copy, `s[a:b]?`")]
#[case("n = f(&(s[:2]))", "declare the parameter `&substr`")]
#[case("n = f(&s[1:]?)", "copy the elements with `xs[a:b]?`")]
#[case("n = f(&\"abc\"[1:])", "a &substr view")]
#[case("v = &s[1:]\nn = f(v)", "or pass an owned copy, `v[:]?`")]
#[case("v = &s[1:]\nn = f(&v)", "or pass an owned copy, `v[:]?`")]
#[case(
    "h = Holder(n=1)\nn = h.take(&s[1:])",
    "a &substr view, but the parameter takes &str"
)]
#[case(
    "xs = [1, 2].unwrap()\nn = total(&xs[1:])",
    "a &[i64] view, but the parameter takes &list[i64]; declare the parameter `&[i64]` to borrow it without copying, or pass an owned copy, `xs[a:b]?`"
)]
#[case(
    "xs = [1, 2].unwrap()\nv = &xs[1:]\nn = total(v)",
    "`xs[a:b]?` from the owner"
)]
#[case(
    "mut xs = [1, 2].unwrap()\nbump(&mut xs[1:])",
    "a &mut [i64] view, but the parameter takes &mut list[i64]; declare the parameter `&mut [i64]` to change the elements in place"
)]
fn views_at_owner_reference_parameters_name_the_mismatch(
    #[case] call: &str,
    #[case] expected: &str,
) {
    let source = format!("{OWNER_PARAMETERS}{call}");
    let error = support::check_diagnostic(&source);
    assert!(error.contains(expected), "{source}\n{error}");
    assert!(!error.contains("use `?`"), "{source}\n{error}");
}

/// Both remedies the mismatch diagnostic names compile (issue #802).
#[test]
fn view_mismatch_remedies_compile() {
    support::assert_stdout(
        r#"
def borrowed(s: &substr) -> i64:
    s.byte_len()

def owned(s: &str) -> i64:
    s.byte_len()

def main() -> Result[(), AllocError | IoError]:
    s = "hello"
    v = &s[1:]
    print(borrowed(&s[1:]), borrowed(s=&s[1:3]), borrowed(v), borrowed(&s))?
    print(owned(s[1:]?), owned(s=s[1:3]?), owned(v[:]?))?
    Ok(())
"#,
        "4 2 4 5\n4 2 4\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn views_add_no_allocations() {
    let output = support::run(
        r#"
def count(text: &substr, scalar: &str) -> Result[i64, OverflowError[i64]]:
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

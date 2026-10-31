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
def count(text: &substr, scalar: &str) -> i64:
    mut n = 0
    for c in text:
        if c == scalar:
            n += 1
    n

def words(text: &str) -> i64:
    mut n = 0
    mut start = 0
    for i, c in enumerate(text):
        if c == " ":
            if count(&text[start:i], "o") > 0:
                n += 1
            start = i + 1
    n

def main() -> Result[(), AllocError | IoError]:
    s = "one two three four"
    print(count(&s[:], "o"), count(&s[4:], "o"), words(&s))?
    Ok(())
"#,
        "3 2 2\n",
    );
}

#[test]
fn view_index_is_checked_against_the_view() {
    let output = support::run(
        r#"
def main() -> Result[(), AllocError | IoError]:
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
    "def g(x: &substr) -> i64:\n    len(x)\n\ns = \"abc\"\nn = g(&s)",
    "pass a whole string as `&s[:]`"
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

#[cfg(feature = "runtime-checks")]
#[test]
fn views_add_no_allocations() {
    let output = support::run(
        r#"
def count(text: &substr, scalar: &str) -> i64:
    mut n = 0
    for c in text:
        if c == scalar:
            n += 1
    n

def main() -> Result[(), AllocError | IoError]:
    s = "a longer heap string, with ünïcode"
    mut sum = 0
    print("__test_begin_no_allocations__")?
    v = &s[2:8]
    sum += count(&s[:], "n") + len(v) + v.byte_len()
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

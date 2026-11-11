//! Signed integer domains used to check a fixed lower bound once.
mod support;
use rstest::rstest;
use support::{assert_stdout as runs, check_error};

#[test]
fn literals_conversions_and_widening() {
    runs(
        r#"
def take(value: Positive[i64] = 2) -> Positive[i64]:
    value
p: Positive[i64] = 3
n: NonNegative[i32] = 0
print(p, n, take(), take(4), Positive.try_from(5).unwrap()).unwrap()
print(Positive.try_from(0).is_err(), Positive.try_from(-1).is_err()).unwrap()
print(NonNegative.try_from(0).unwrap(), NonNegative.try_from(-1).is_err()).unwrap()
a: NonZero[i64] = NonZero.from(p)
b: NonNegative[i64] = NonNegative.from(p)
print(a, b, list(range(0, 7, a)).unwrap()).unwrap()
print(NonZero[i64].from(p), NonNegative[i64].from(p)).unwrap()
"#,
        "3 0 2 4 5\nTrue True\n0 True\n3 3 [0, 3, 6]\n3 3\n",
    );
}

#[test]
fn arithmetic_keeps_only_valid_guarantees() {
    runs(
        r#"
def main() -> Result[(), IoError | OverflowError[i8]]:
    a: Positive[i8] = 3
    b: Positive[i8] = 2
    sum: Positive[i8] = a + b
    product: Positive[i8] = a * b
    difference: i8 = a - b
    negative: i8 = -a
    mut n: NonNegative[i8] = 0
    n += 2
    n *= 3
    mut p = a
    p += b
    p *= 2
    print(sum, product, difference, negative, n, p)?
    print(a == 3, a > b, n >= 0)?
    overflow = checked:
        a * 100
    print(overflow.is_err())?
    addition = checked:
        p + 120
    print(addition.is_err())?
    Ok(())
"#,
        "5 6 1 -3 6 10\nTrue True True\nTrue\nTrue\n",
    );
}

#[test]
fn formatting_hashing_and_copy() {
    runs(
        r#"
p: Positive[i64] = 42
n: NonNegative[i64] = 0
print(f"{p:>4} {p:x} {n:+} {p!r}".unwrap()).unwrap()
print(str.from(p).unwrap(), str.from(n).unwrap()).unwrap()
keys = {p: "answer"}.unwrap()
seen = {n}.unwrap()
values: list[Positive[i64]] = [1, 2].unwrap()
print(keys[p], n in seen, values, p).unwrap()
"#,
        "  42 2a +0 42\n42 0\nanswer True [1, 2] 42\n",
    );
}

#[test]
fn borrowed_domains_and_checked_error_payloads() {
    runs(
        r#"
def negate(value: &Positive[i8]) -> Result[i8, OverflowError[i8]]:
    Ok(-value)
def increment(value: &NonNegative[i8]) -> Result[NonNegative[i8], OverflowError[i8]]:
    Ok(value + 1)
def overflow(value: Positive[i8]) -> Result[Positive[i8], OverflowError[i8]]:
    checked:
        value * 2
def main() -> Result[(), IoError | OverflowError[i8]]:
    p: Positive[i8] = 100
    n: NonNegative[i8] = 0
    print(negate(&p)?, increment(&n)?)?
    match overflow(p):
        case Ok(_):
            print("unexpected")?
        case Err(OverflowError(wrapped)):
            print(wrapped)?
    Ok(())
"#,
        "-100 1\n-56\n",
    );
}

#[rstest]
fn non_integer_elements_have_no_replacement_hint(
    #[values("Positive", "NonNegative")] domain: &str,
    #[values("f32", "f64", "bool", "str", "list[i64]")] element: &str,
) {
    // Issue #700: NonZero[float] is not a valid alternative.
    let source =
        format!("def f(x: {domain}[{element}]) -> ():\n    pass\ndef main() -> ():\n    pass\n");
    let error = plenty::check_source(&source).unwrap_err().to_string();
    assert!(
        error.ends_with(&format!("`{domain}` requires a signed integer type")),
        "{error}"
    );
}

#[rstest]
fn unsigned_elements_keep_valid_replacement_hints(
    #[values("Positive", "NonNegative")] domain: &str,
    #[values("u8", "u16", "u32", "u64")] element: &str,
) {
    let source =
        format!("def f(x: {domain}[{element}]) -> ():\n    pass\ndef main() -> ():\n    pass\n");
    let error = plenty::check_source(&source).unwrap_err().to_string();
    let suggestion = if domain == "Positive" {
        format!("NonZero[{element}]")
    } else {
        element.to_owned()
    };
    assert!(
        error.ends_with(&format!(
            "`{domain}` requires a signed integer type; for `{element}`, use `{suggestion}`"
        )),
        "{error}"
    );
}

#[rstest]
#[case("p: Positive[i64] = 0", "must be greater than zero")]
#[case("p: Positive[i64] = -1", "must be greater than zero")]
#[case("n: NonNegative[i8] = -1", "cannot be negative")]
#[case("p: Positive[i8] = 128", "out of range")]
#[case("p: Positive[u64] = 1", "use `NonZero[u64]`")]
#[case("n: NonNegative[u32] = 0", "use `u32`")]
#[case("p = Positive(3)", "Positive.try_from(value)?")]
#[case("p: Positive[i64] = 3\nprint(p.value).unwrap()", "field is private")]
#[case(
    "p: Positive[i64] = 3\nx = 2\nprint(p + x).unwrap()",
    "only between two"
)]
#[case(
    "p: Positive[i64] = 3\nprint(p * 0).unwrap()",
    "must be greater than zero"
)]
#[case("p: Positive[i64] = 3\nprint(p // p).unwrap()", ".get()")]
#[case("p: Positive[i64] = 3\nprint(~p).unwrap()", ".get()")]
#[case("mut p: Positive[i64] = 3\np -= 1", ".get()")]
#[case("p = NonZero.from(3)", "takes a Positive")]
#[case("class Positive:\n    value: i64", "Positive")]
fn invalid_domains_are_rejected(#[case] source: &str, #[case] message: &str) {
    let error = check_error(source);
    assert!(
        error.contains(message),
        "expected {message:?}, got {error:?}"
    );
}

const OPERATORS: [&str; 9] = ["*", "+", "-", "<", "<=", ">", ">=", "==", "!="];

fn reason(domain: &str) -> &'static str {
    match domain {
        "NonZero" => "a NonZero value cannot be zero",
        "Positive" => "a Positive value must be greater than zero",
        _ => "a NonNegative value cannot be negative",
    }
}

/// `source` as a program whose `main` admits the overflow a negated suffixed
/// literal can report, so that failure never hides the diagnostic under test.
fn fallible(source: &str) -> String {
    let program = support::program(source);
    let body = program
        .strip_suffix("    pass\n")
        .expect("a fragment ends in `pass`");
    body.replace(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64] | OverflowError[i32]]:",
    ) + "    Ok(())\n"
}

/// Every place a literal beside a domain value is checked, without the
/// binding: operands on either side of each operator, and `*=`/`+=` on a
/// local, a field, a list element and a dereferenced `&mut`.
fn literal_positions(domain: &str, element: &str, literal: &str) -> Vec<String> {
    let ty = format!("{domain}[{element}]");
    let mut sources = Vec::new();
    for op in OPERATORS {
        sources.push(format!("a: {ty} = 3\nprint(a {op} {literal}).unwrap()"));
        sources.push(format!("a: {ty} = 3\nprint({literal} {op} a).unwrap()"));
    }
    let updates: &[&str] = if domain == "NonZero" {
        &["*="]
    } else {
        &["*=", "+="]
    };
    for op in updates {
        sources.push(format!("mut a: {ty} = 3\na {op} {literal}"));
        sources.push(format!(
            "class Holder:\n    value: {ty}\nmut h = Holder(3)\nh.value {op} {literal}"
        ));
        sources.push(format!(
            "mut xs: list[{ty}] = [3].unwrap()\nxs[0] {op} {literal}"
        ));
        sources.push(format!(
            "def update(r: &mut {ty}) -> ():\n    *r {op} {literal}\nmut a: {ty} = 3\nupdate(&mut a)"
        ));
    }
    sources
}

#[rstest]
#[case("NonZero", "i64", &["0i64", "-0i64", "-(0i64)", "-((0i64))", "(-(0i64))", "-(0)", "(0)"])]
#[case("NonZero", "i8", &["-(0i8)", "(0i8)"])]
#[case("NonZero", "u8", &["(0u8)", "((0u8))"])]
#[case("Positive", "i64", &["0i64", "-1i64", "-(0i64)", "-(1i64)", "-((1i64))", "(-(1i64))", "-(1)", "-(9223372036854775808i64)"])]
#[case("Positive", "i8", &["-(1i8)", "-(128i8)", "(0i8)"])]
#[case("NonNegative", "i64", &["-1i64", "-(1i64)", "-((1i64))", "(-(1i64))", "-(1)", "-(9223372036854775808i64)"])]
#[case("NonNegative", "i32", &["-(1i32)", "-(2147483648i32)"])]
fn out_of_domain_literal_is_rejected_in_every_position(
    #[case] domain: &str,
    #[case] element: &str,
    #[case] literals: &[&str],
) {
    // Issue #955: a parenthesized negation lowers as `0 - x`; its value was
    // never checked, so `d *= -(0i64)` stored zero in a `NonZero`.
    for literal in literals {
        for source in literal_positions(domain, element, literal) {
            let error = plenty::check_source(&support::program(&source))
                .expect_err(&source)
                .to_string();
            assert!(error.contains(reason(domain)), "{source}\n{error}");
        }
    }
}

#[rstest]
#[case("NonZero", "i64", &["-(0i32)", "-(1i32)", "(1u8)", "2i8"], "expected i64, got ")]
#[case("Positive", "i64", &["-(1i32)", "(1i32)", "1i16"], "expected i64, got ")]
#[case("NonNegative", "i8", &["-(1i64)", "(0i64)", "0i16"], "expected i8, got ")]
#[case("NonZero", "i8", &["128", "-(129)", "-(129i8)", "(128i8)"], "out of range")]
#[case("Positive", "i8", &["(128)", "128i8"], "out of range")]
// Negation applied twice is an operation, not a literal: it is an ordinary
// integer operand, which each position rejects in its own words.
#[case("NonZero", "i64", &["-(-(0i64))", "-(-(1i64))", "-(-1i64)", "-(-(0))"], "")]
#[case("Positive", "i64", &["-(-(1i64))", "-(-1)"], "")]
#[case("NonNegative", "i64", &["-(-(1i64))", "-(-(0i64))"], "")]
fn literal_of_another_width_or_shape_is_rejected(
    #[case] domain: &str,
    #[case] element: &str,
    #[case] literals: &[&str],
    #[case] message: &str,
) {
    for literal in literals {
        for source in literal_positions(domain, element, literal) {
            let error = plenty::check_source(&support::program(&source))
                .expect_err(&source)
                .to_string();
            assert!(error.contains(message), "{source}\n{error}");
        }
    }
}

#[rstest]
fn suffixed_literal_does_not_convert_in_a_binding(
    #[values("NonZero", "Positive", "NonNegative")] domain: &str,
    #[values("0i64", "1i64", "-(0i64)", "-(1i64)", "-(-(1i64))", "-(0i32)")] literal: &str,
) {
    // Only an unsuffixed literal converts where a domain type is expected; a
    // suffixed one is an `i64` whatever its value.
    for source in [
        format!("a: {domain}[i64] = {literal}"),
        format!("def take(a: {domain}[i64]) -> ():\n    pass\ntake({literal})"),
        format!("xs: list[{domain}[i64]] = [{literal}].unwrap()"),
        format!("class Holder:\n    value: {domain}[i64]\nh = Holder({literal})"),
    ] {
        let error = plenty::check_source(&fallible(&source))
            .expect_err(&source)
            .to_string();
        assert!(
            error.contains(&format!("{domain}[i64]")) && error.contains("expected "),
            "{source}\n{error}"
        );
    }
}

#[test]
fn unsuffixed_out_of_domain_literal_is_rejected_in_a_binding() {
    for (ty, literal) in [
        ("NonZero[i64]", "-(0)"),
        ("NonZero[i64]", "(0)"),
        ("NonZero[i64]", "-((0))"),
        ("Positive[i64]", "-(1)"),
        ("Positive[i64]", "-(0)"),
        ("NonNegative[i64]", "-(1)"),
        ("NonNegative[i64]", "-((1))"),
    ] {
        let domain = ty.split('[').next().unwrap();
        for source in [
            format!("a: {ty} = {literal}"),
            format!("def take(a: {ty} = {literal}) -> ():\n    pass\ntake()"),
            format!("xs: list[{ty}] = [1, {literal}].unwrap()"),
            format!("class Holder:\n    value: {ty}\nh = Holder({literal})"),
            format!("const BAD: {ty} = {literal}\ndef main() -> ():\n    pass\n"),
        ] {
            let error = plenty::check_source(&support::program(&source))
                .expect_err(&source)
                .to_string();
            assert!(
                error.contains(reason(domain)) || error.contains(&format!("not a `{ty}`")),
                "{source}\n{error}"
            );
        }
    }
}

#[test]
fn parenthesized_and_suffixed_literals_in_the_domain_are_accepted() {
    // The checked constant is what the operation uses, so a negated literal
    // adds no failure of its own: `OverflowError` here is the product's.
    runs(
        r#"
def main() -> Result[(), IoError | OverflowError[i64] | OverflowError[i8]]:
    a: NonZero[i64] = 3
    mut b = a
    b *= -(2i64)
    b *= ((-(1)))
    print(a * -(1i64), -(2i64) * a, -((2)) * a, a * (4i64), b)?
    print(a == -(3i64), a != -(3i64), a > -(1i64), -(4i64) < a, a + -(3i64), a - (3i64))?
    low: NonZero[i64] = -9223372036854775808
    print(low == -(9223372036854775808i64), low == -9223372036854775808i64, low < -(1))?
    narrow: NonZero[i8] = -128
    print(narrow == -(128i8), narrow)?
    p: Positive[i64] = 3
    mut q = p
    q *= ((2i64))
    q += (1i64)
    print(p * (2i64), (1i64) + p, q, p > (2i64), p - (5i64))?
    n: NonNegative[i64] = 4
    mut m = n
    m += -(0i64)
    m *= (0i64)
    print(n + -(0i64), n * (0), m, n >= -(0i64), n - (9i64))?
    Ok(())
"#,
        "-3 -6 -6 12 6\nFalse True True True 0 0\nTrue True True\nTrue -128\n6 4 7 True -2\n4 0 0 True -5\n",
    );
}

#[test]
fn the_reported_programs_are_rejected() {
    // Issue #955: the first printed 0 and died with SIGFPE in `7 % d`; the
    // second printed -3 from a `Positive[i64]`.
    let error = check_error(
        "def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    mut d: NonZero[i64] = 3\n    d *= -(0i64)\n    print(d)?\n    print(7 % d)?\n    Ok(())\n",
    );
    assert!(
        error.contains("3:10: `0` is not a `NonZero[i64]`: a NonZero value cannot be zero"),
        "{error}"
    );
    let error = check_error(
        "def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    p: Positive[i64] = 3\n    q = p * -(1i64)\n    print(q)?\n    Ok(())\n",
    );
    assert!(
        error.contains(
            "3:13: literal is not a `Positive[i64]`: a Positive value must be greater than zero"
        ),
        "{error}"
    );
}

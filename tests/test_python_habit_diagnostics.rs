//! Diagnostics for Python spellings that Plenty rejects: each names the
//! unsupported form and its replacement at the offending token (issue #38).
mod support;

fn diagnostic(source: &str) -> String {
    support::check_source(source).unwrap_err().to_string()
}

fn output(source: &str) -> String {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const POWER: &str = "exponentiation with `**` is not supported; use `base.pow(exponent)`";
const REPETITION: &str = "lists do not support `*` repetition; build the list with a comprehension, such as `[value for _ in range(count)]?`";

#[test]
fn power_operator_names_the_pow_method() {
    for (expression, column) in [
        ("n ** 2", 11),
        ("n**2", 10),
        ("2 ** n", 11),
        ("x ** 0.5", 11),
        ("n ** (n + 1)", 11),
        ("n ** -1", 11),
    ] {
        let source = format!("def main() -> ():\n    n = 3\n    x = 2.0\n    m = {expression}\n");
        assert_eq!(diagnostic(&source), format!("4:{column}: {POWER}"));
    }
}

#[test]
fn multiplying_by_a_dereference_is_still_valid() {
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    n = 3\n    r = &n\n    print(n ** r)?\n    print(n * *r)?\n    Ok(())\n"
        ),
        "9\n9\n"
    );
    // Separated stars are a multiplication and a dereference, not `**`.
    assert_eq!(
        diagnostic("def main() -> ():\n    n = 3\n    m = n * * 2\n"),
        "3:15: dereference requires a reference or a Box"
    );
    assert_eq!(
        diagnostic("def main() -> ():\n    n = 3\n    m = n ** missing\n"),
        "3:14: unknown binding `missing`"
    );
}

#[test]
fn several_names_bind_from_an_unparenthesized_tuple() {
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    n = 3\n    lo, hi = 0, n\n    print((lo, hi))?\n    Ok(())\n"
        ),
        "(0, 3)\n"
    );
}

#[test]
fn mixed_numeric_operands_name_the_conversion() {
    for (body, expected) in [
        (
            "n = 3\n    m = n * 1.5",
            "3:9: cannot combine i64 with f64; convert with `f64(n)`",
        ),
        (
            "n = 3\n    m = 1.5 * n",
            "3:15: cannot combine f64 with i64; convert with `f64(n)`",
        ),
        (
            "b = 3 == 3.0",
            "2:9: cannot combine i64 with f64; convert with `f64(3)`",
        ),
        (
            "n = 3\n    x = 2.5\n    b = n < x",
            "4:9: cannot combine i64 with f64; convert with `f64(n)`",
        ),
        (
            "n = 3\n    x = 2.5\n    m = x + n",
            "4:13: cannot combine f64 with i64; convert with `f64(n)`",
        ),
        (
            "n: i32 = 3\n    k: i64 = 4\n    m = n + k",
            "4:9: cannot combine i32 with i64; convert with `i64(n)`",
        ),
        (
            "a: u64 = 3\n    b: i64 = 4\n    m = a + b",
            "4:13: cannot combine u64 with i64; convert with `u64(b)`",
        ),
        (
            "n = 3\n    x = 1.5\n    m = (n + 1) * x",
            "4:9: cannot combine i64 with f64; convert with `f64(...)`",
        ),
        (
            "n = 3\n    m: f64 = n * 1.5",
            "3:14: cannot combine i64 with f64; convert with `f64(n)`",
        ),
    ] {
        let source = format!("def main() -> ():\n    {body}\n");
        assert_eq!(diagnostic(&source), expected, "{body}");
    }
}

#[test]
fn operands_that_agree_still_report_the_annotation_mismatch() {
    assert_eq!(
        diagnostic("def main() -> ():\n    n = 3\n    m: f64 = n * 2\n"),
        "3:14: expected f64, got i64"
    );
    assert_eq!(
        diagnostic("def main() -> ():\n    a: i32 = 3\n    m: i64 = a * a\n"),
        "3:14: expected i64, got i32"
    );
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    n = 3\n    print(f64(n) * 1.5)?\n    Ok(())\n"
        ),
        "4.5\n"
    );
}

#[test]
fn list_repetition_names_the_comprehension() {
    for (body, column) in [
        ("xs = [0.0] * n", 10),
        ("xs = [0.0]? * n", 10),
        ("xs = ([0.0] * n)?", 11),
        ("xs = n * [0]", 14),
        ("xs: list[f64] = [0.0] * n", 21),
        ("xs = [0] * 3", 10),
        ("xs = ys * n", 10),
        ("xs = ys * 3", 10),
    ] {
        let source =
            format!("def main() -> Result[(), Failure]:\n    n = 3\n    ys = [1, 2]?\n    {body}\n    Ok(())\n");
        assert_eq!(
            diagnostic(&source),
            format!("4:{column}: {REPETITION}"),
            "{body}"
        );
    }
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    n = 3\n    print([0.0 for _ in range(n)]?)?\n    Ok(())\n"
        ),
        "[0.0, 0.0, 0.0]\n"
    );
}

#[test]
fn multiplying_two_lists_keeps_the_operator_diagnostic() {
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), Failure]:\n    xs = [1]?\n    ys = [2]?\n    zs = xs * ys\n    Ok(())\n"
        ),
        "4:10: operator `*` does not accept list[i64]"
    );
}

#[test]
fn str_cast_names_str_from() {
    let expected = "only numeric types support cast syntax; format a number or boolean as text with `str.from(value)?`";
    assert_eq!(
        diagnostic("def main() -> ():\n    s = str(42)\n"),
        format!("2:9: {expected}")
    );
    assert_eq!(
        diagnostic("type Text = str\ndef main() -> ():\n    s = Text(4.5)\n"),
        format!("3:9: {expected}")
    );
    assert_eq!(
        diagnostic("def main() -> ():\n    b = bool(1)\n"),
        "2:9: only numeric types support cast syntax"
    );
    assert_eq!(
        output("def main() -> Result[(), Failure]:\n    print(str.from(42)?)?\n    Ok(())\n"),
        "42\n"
    );
}

#[test]
fn empty_display_names_the_annotation_and_the_typed_constructor() {
    let dict = "empty collection needs a type; annotate the binding, as in `name: dict[str, i64] = {}?`, or use a typed constructor, as in `dict[str, i64]()?`";
    let list = "empty collection needs a type; annotate the binding, as in `name: list[i64] = []?`, or use a typed constructor, as in `list[i64]()?`";
    for (body, expected) in [
        ("seen = {}", dict),
        ("seen = {}?", dict),
        ("seen = []", list),
        ("seen = []?", list),
    ] {
        let source = format!("def main() -> Result[(), Failure]:\n    {body}\n    Ok(())\n");
        assert_eq!(diagnostic(&source), format!("2:12: {expected}"), "{body}");
    }
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    seen: dict[str, i64] = {}?\n    items: list[i64] = []?\n    print((len(seen), len(items), len(dict[str, i64]()?), len(list[i64]()?)))?\n    Ok(())\n"
        ),
        "(0, 0, 0, 0)\n"
    );
}

#[test]
fn float_patterns_name_the_conditional_chain() {
    for pattern in ["2.5", "-2.5", "1e3", "2f64"] {
        let source = format!(
            "def main() -> ():\n    s = 2.5\n    match s:\n        case {pattern}:\n            pass\n        case _:\n            pass\n"
        );
        assert_eq!(
            diagnostic(&source),
            "4:14: a pattern cannot be a float; compare floats with an `if`/`elif` chain",
            "{pattern}"
        );
    }
}

#[test]
fn matching_an_unsupported_type_names_the_conditional_chain() {
    for value in ["2.5", "[1, 2]?", "{1: 2}?"] {
        let source = format!(
            "def main() -> Result[(), Failure]:\n    s = {value}\n    match s:\n        case _:\n            pass\n    Ok(())\n"
        );
        assert_eq!(
            diagnostic(&source),
            "3:11: match requires an enum, integer, `str`, or `bool` value, or a reference to one; compare other values with an `if`/`elif` chain",
            "{value}"
        );
    }
}

#[test]
fn mut_payload_binding_names_the_working_spellings() {
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), Failure]:\n    found = Some([1, 2]?)\n    match found:\n        case Some(mut items):\n            items.append(3)?\n        case Nothing:\n            pass\n    Ok(())\n"
        ),
        "4:19: payload bindings cannot be declared `mut`; bind the payload and move it into a mutable local in the arm, as in `mut local = payload`, or match `&mut value` to change it in place"
    );
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    found = Some([1, 2]?)\n    match found:\n        case Some(items):\n            mut local = items\n            local.append(3)?\n            print(local)?\n        case Nothing:\n            pass\n    Ok(())\n"
        ),
        "[1, 2, 3]\n"
    );
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    mut found = Some([1, 2]?)\n    match &mut found:\n        case Some(items):\n            items.append(3)?\n        case Nothing:\n            pass\n    print(found)?\n    Ok(())\n"
        ),
        "Option[list[i64]].Some([1, 2, 3])\n"
    );
}

#[test]
fn a_binding_that_left_scope_explains_block_scope() {
    let hint = |name: &str| {
        format!("unknown binding `{name}`; a binding declared inside a block ends with that block; bind `mut {name}` to a value before the block and assign it inside")
    };
    for (body, position, name) in [
        (
            "if c:\n        y = 1\n    else:\n        y = 2\n    z = y",
            "7:9",
            "y",
        ),
        (
            "for i in range(3):\n        y = i\n    z = y",
            "5:9",
            "y",
        ),
        (
            "for i in range(3):\n        pass\n    z = i",
            "5:9",
            "i",
        ),
        (
            "while c:\n        y = 1\n        break\n    z = &y",
            "6:10",
            "y",
        ),
        (
            "match Some(1):\n        case Some(v):\n            pass\n        case Nothing:\n            pass\n    z = v",
            "8:9",
            "v",
        ),
        (
            "if c:\n        y = 1\n    else:\n        z = y",
            "6:13",
            "y",
        ),
    ] {
        let source = format!("def main() -> ():\n    c = True\n    {body}\n");
        assert_eq!(
            diagnostic(&source),
            format!("{position}: {}", hint(name)),
            "{body}"
        );
    }
    assert_eq!(
        output(
            "def main() -> Result[(), Failure]:\n    c = True\n    mut y = 0\n    if c:\n        y = 1\n    else:\n        y = 2\n    print(y)?\n    Ok(())\n"
        ),
        "1\n"
    );
}

#[test]
fn a_name_never_bound_has_no_scope_hint() {
    assert_eq!(
        diagnostic("def main() -> ():\n    z = y\n"),
        "2:9: unknown binding `y`"
    );
    assert_eq!(
        diagnostic("def main() -> ():\n    z = y\n    if True:\n        y = 1\n"),
        "2:9: unknown binding `y`"
    );
    // Another function's locals are not this function's earlier bindings.
    assert_eq!(
        diagnostic(
            "def other() -> ():\n    if True:\n        y = 1\ndef main() -> ():\n    z = y\n"
        ),
        "5:9: unknown binding `y`"
    );
}

#[test]
fn a_docstring_only_str_function_explains_the_docstring_rule() {
    let expected = "2:5: expected str, got (); a string at the beginning of a function is its documentation; write `return` before it to return the text";
    assert_eq!(
        diagnostic("def f() -> str:\n    \"hi\"\ndef main() -> ():\n    pass\n"),
        expected
    );
    assert_eq!(
        diagnostic("def f() -> str:\n    \"\"\"hi\"\"\"\ndef main() -> ():\n    pass\n"),
        expected
    );
    assert_eq!(
        output(
            "def f() -> str:\n    return \"hi\"\ndef main() -> Result[(), Failure]:\n    print(f())?\n    Ok(())\n"
        ),
        "hi\n"
    );
}

#[test]
fn other_missing_results_keep_the_plain_diagnostic() {
    assert_eq!(
        diagnostic("def f() -> i64:\n    \"doc\"\ndef main() -> ():\n    pass\n"),
        "1:1: expected i64, got ()"
    );
    assert_eq!(
        diagnostic("def f() -> str:\n    pass\ndef main() -> ():\n    pass\n"),
        "1:1: expected str, got ()"
    );
    support::check_source("def f() -> ():\n    \"doc\"\ndef main() -> ():\n    f()\n").unwrap();
}

#[test]
fn keyword_arguments_name_positional_passing() {
    let declarations = "class Lcg:\n    state: i64\n    def next(self, step: i64) -> i64:\n        self.state + step\ndef add(a: i64, b: i64) -> i64:\n    a + b\n";
    for (body, position, name) in [
        ("g = Lcg(state=1)", "8:13", "state"),
        ("g = Lcg(state = 1)", "8:13", "state"),
        ("n = add(1, b=2)", "8:16", "b"),
        ("g = Lcg(1)\n    n = g.next(step=2)", "9:16", "step"),
        ("print(1, end=\"\").unwrap()", "8:14", "end"),
    ] {
        let source = format!("{declarations}def main() -> ():\n    {body}\n");
        assert_eq!(
            diagnostic(&source),
            format!("{position}: keyword arguments are not supported; pass the value by position, without `{name}=`"),
            "{body}"
        );
    }
}

#[test]
fn other_argument_errors_are_unchanged() {
    assert_eq!(
        diagnostic(
            "def add(a: i64, b: i64) -> i64:\n    a + b\ndef main() -> ():\n    n = add(1 2)\n"
        ),
        "4:15: expected `,`"
    );
    // A comparison beginning with a name is an ordinary argument.
    assert_eq!(
        output(
            "def flag(on: bool) -> bool:\n    on\ndef main() -> Result[(), Failure]:\n    a = 1\n    print(flag(a == 1))?\n    Ok(())\n"
        ),
        "True\n"
    );
}

#[test]
fn membership_accepts_a_borrowed_string() {
    assert_eq!(
        output(
            "def in_set(regs: &set[str], x: &str) -> bool:\n    x in regs\ndef in_list(regs: &list[str], x: &str) -> bool:\n    x in regs\ndef in_dict(regs: &dict[str, i64], x: &str) -> bool:\n    x not in regs\ndef main() -> Result[(), Failure]:\n    names = {\"a\", \"b\"}?\n    listed = [\"a\"]?\n    table = {\"a\": 1}?\n    print((in_set(&names, \"a\"), in_list(&listed, \"b\"), in_dict(&table, \"a\")))?\n    Ok(())\n"
        ),
        "(True, False, False)\n"
    );
}

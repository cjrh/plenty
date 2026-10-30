//! Diagnostics for Python spellings that Plenty rejects: each names the
//! unsupported form and its replacement at the offending token (issue #38).
mod support;
use support::check_diagnostic as diagnostic;

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
            "def main() -> Result[(), IoError]:\n    n = 3\n    r = &n\n    print(n ** r)?\n    print(n * *r)?\n    Ok(())\n"
        ),
        "9\n9\n"
    );
    // Separated stars are a multiplication and a dereference, not `**`.
    assert_eq!(
        diagnostic("def main() -> ():\n    n = 3\n    m = n * * 2\n"),
        "3:15: dereference requires a reference, a Box, or a Shared handle"
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
            "def main() -> Result[(), IoError]:\n    n = 3\n    lo, hi = 0, n\n    print((lo, hi))?\n    Ok(())\n"
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
            "4:13: cannot combine u64 with i64; no integer type holds both",
        ),
        (
            "a: i8 = 3\n    b: u16 = 4\n    m = a + b",
            "4:9: cannot combine i8 with u16; convert both with `i32(a)` and `i32(b)`",
        ),
        (
            "a: u8 = 3\n    b: i8 = 4\n    m = a + b",
            "4:9: cannot combine u8 with i8; convert both with `i16(a)` and `i16(b)`",
        ),
        (
            "a: u16 = 3\n    b: i32 = 4\n    m = a + b",
            "4:9: cannot combine u16 with i32; convert with `i32(a)`",
        ),
        (
            "a: i64 = 3\n    b: u64 = 4\n    m = a + b",
            "4:9: cannot combine i64 with u64; no integer type holds both",
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
            "def main() -> Result[(), AllocError | IoError]:\n    n = 3\n    print(f64(n) * 1.5)?\n    Ok(())\n"
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
        let source = format!(
            "def main() -> Result[(), AllocError | IoError]:\n    n = 3\n    ys = [1, 2]?\n    {body}\n    Ok(())\n"
        );
        assert_eq!(
            diagnostic(&source),
            format!("4:{column}: {REPETITION}"),
            "{body}"
        );
    }
    assert_eq!(
        output(
            "def main() -> Result[(), AllocError | IoError]:\n    n = 3\n    print([0.0 for _ in range(n)]?)?\n    Ok(())\n"
        ),
        "[0.0, 0.0, 0.0]\n"
    );
}

#[test]
fn multiplying_two_lists_keeps_the_operator_diagnostic() {
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), AllocError | IoError]:\n    xs = [1]?\n    ys = [2]?\n    zs = xs * ys\n    Ok(())\n"
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
        output(
            "def main() -> Result[(), AllocError | IoError]:\n    print(str.from(42)?)?\n    Ok(())\n"
        ),
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
        let source =
            format!("def main() -> Result[(), AllocError | IoError]:\n    {body}\n    Ok(())\n");
        assert_eq!(diagnostic(&source), format!("2:12: {expected}"), "{body}");
    }
    assert_eq!(
        output(
            "def main() -> Result[(), AllocError | IoError]:\n    seen: dict[str, i64] = {}?\n    items: list[i64] = []?\n    print((len(seen), len(items), len(dict[str, i64]()?), len(list[i64]()?)))?\n    Ok(())\n"
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
            "def main() -> Result[(), AllocError | IoError]:\n    s = {value}\n    match s:\n        case _:\n            pass\n    Ok(())\n"
        );
        assert_eq!(
            diagnostic(&source),
            "3:11: match requires an enum, class, integer, `str`, or `bool` value, or a reference to one; compare other values with an `if`/`elif` chain",
            "{value}"
        );
    }
}

#[test]
fn mut_payload_binding_names_the_working_spellings() {
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), AllocError | IoError]:\n    found = Some([1, 2]?)\n    match found:\n        case Some(mut items):\n            items.append(3)?\n        case Nothing:\n            pass\n    Ok(())\n"
        ),
        "4:19: payload bindings cannot be declared `mut`; bind the payload and move it into a mutable local in the arm, as in `mut local = payload`, or match `&mut value` to change it in place"
    );
    assert_eq!(
        output(
            "def main() -> Result[(), AllocError | IoError]:\n    found = Some([1, 2]?)\n    match found:\n        case Some(items):\n            mut local = items\n            local.append(3)?\n            print(local)?\n        case Nothing:\n            pass\n    Ok(())\n"
        ),
        "[1, 2, 3]\n"
    );
    assert_eq!(
        output(
            "def main() -> Result[(), AllocError | IoError]:\n    mut found = Some([1, 2]?)\n    match &mut found:\n        case Some(items):\n            items.append(3)?\n        case Nothing:\n            pass\n    print(found)?\n    Ok(())\n"
        ),
        "Option[list[i64]].Some([1, 2, 3])\n"
    );
}

#[test]
fn a_binding_that_left_scope_explains_block_scope() {
    let hint = |name: &str| {
        format!(
            "unknown binding `{name}`; a binding declared inside a block ends with that block; bind `mut {name}` to a value before the block and assign it inside"
        )
    };
    for (body, position, name) in [
        (
            "if c:\n        y = 1\n    else:\n        y = 2\n    z = y",
            "7:9",
            "y",
        ),
        ("for i in range(3):\n        y = i\n    z = y", "5:9", "y"),
        ("for i in range(3):\n        pass\n    z = i", "5:9", "i"),
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
            "def main() -> Result[(), IoError]:\n    c = True\n    mut y = 0\n    if c:\n        y = 1\n    else:\n        y = 2\n    print(y)?\n    Ok(())\n"
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
            "def f() -> str:\n    return \"hi\"\ndef main() -> Result[(), IoError]:\n    print(f())?\n    Ok(())\n"
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
fn named_arguments_are_accepted_for_user_defined_signatures() {
    let declarations = "class Lcg:\n    state: i64\n    def next(self, step: i64) -> i64:\n        self.state + step\ndef add(a: i64, b: i64) -> i64:\n    a + b\n";
    for body in [
        "g = Lcg(state=1)",
        "g = Lcg(state = 1)",
        "n = add(1, b=2)",
        "g = Lcg(1)\n    n = g.next(step=2)",
    ] {
        let source = format!("{declarations}def main() -> ():\n    {body}\n");
        support::check_source(&source).unwrap_or_else(|error| panic!("{body}: {error}"));
    }
}

#[test]
fn python_print_end_parameter_remains_unsupported() {
    let error = diagnostic("def main() -> ():\n    print(1, end=\"\").unwrap()\n");
    assert!(
        error.contains("named") || error.contains("keyword"),
        "{error}"
    );
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
            "def flag(on: bool) -> bool:\n    on\ndef main() -> Result[(), AllocError | IoError]:\n    a = 1\n    print(flag(a == 1))?\n    Ok(())\n"
        ),
        "True\n"
    );
}

#[test]
fn membership_accepts_a_borrowed_string() {
    assert_eq!(
        output(
            "def in_set(regs: &set[str], x: &str) -> bool:\n    x in regs\ndef in_list(regs: &list[str], x: &str) -> bool:\n    x in regs\ndef in_dict(regs: &dict[str, i64], x: &str) -> bool:\n    x not in regs\ndef main() -> Result[(), AllocError | IoError]:\n    names = {\"a\", \"b\"}?\n    listed = [\"a\"]?\n    table = {\"a\": 1}?\n    print((in_set(&names, \"a\"), in_list(&listed, \"b\"), in_dict(&table, \"a\")))?\n    Ok(())\n"
        ),
        "(True, False, False)\n"
    );
}

#[test]
fn none_names_nothing_and_unit() {
    // Issue #570.
    assert_eq!(
        diagnostic("def main() -> ():\n    x = None\n"),
        "2:9: Plenty has no `None`; an absent value is `Nothing`, the empty variant of `Option[T]`"
    );
    assert_eq!(
        diagnostic("def f() -> None:\n    pass\ndef main() -> ():\n    f()\n"),
        "1:12: Plenty has no `None` type; write `()` for no result, or `Option[T]` for an optional value"
    );
    assert_eq!(
        output(
            "def main() -> Result[(), IoError]:\n    x: Option[i64] = Nothing\n    print(x)?\n    Ok(())\n"
        ),
        "Option[i64].Nothing\n"
    );
}

#[test]
fn exception_statements_name_result_and_propagation() {
    // Issue #571.
    let hint = |keyword: &str| {
        format!(
            "`{keyword}` is not supported; Plenty has no exceptions: return `Result[T, E]`, propagate an error with `?`, and handle it with `match`"
        )
    };
    for (body, keyword) in [
        ("try:\n        pass", "try"),
        ("except:\n        pass", "except"),
        ("except ValueError as error:\n        pass", "except"),
        ("finally:\n        pass", "finally"),
        ("raise ValueError(\"bad\")", "raise"),
    ] {
        let source = format!("def main() -> ():\n    {body}\n");
        assert_eq!(
            diagnostic(&source),
            format!("2:5: {}", hint(keyword)),
            "{body}"
        );
    }
    // Only `try` is reserved; the other words remain ordinary names.
    support::check_source(
        "def raise(n: i64) -> i64:\n    n\ndef main() -> ():\n    finally = 2\n    except = raise(1)\n    x = raise(finally) + except\n",
    )
    .unwrap();
}

#[test]
fn python_type_names_name_the_plenty_type() {
    // Issue #572.
    for (written, hint) in [
        ("int", "use `i64`, or another width such as `i32` or `u8`"),
        ("float", "use `f64` or `f32`"),
        ("List", "write `list[T]`"),
        ("Dict", "write `dict[K, V]`"),
        ("Set", "write `set[T]`"),
        ("Tuple", "write `tuple[A, B]`"),
        ("Optional", "write `Option[T]`"),
        ("Union", "write the members as `A | B`"),
    ] {
        // Both the function signature and the local annotation paths.
        assert_eq!(
            diagnostic(&format!(
                "def f(x: {written}) -> ():\n    pass\ndef main() -> ():\n    pass\n"
            )),
            format!("1:10: unknown type `{written}`; {hint}"),
        );
        assert_eq!(
            diagnostic(&format!("def main() -> ():\n    x: {written} = 3\n")),
            format!("2:8: unknown type `{written}`; {hint}"),
        );
    }
    assert_eq!(
        diagnostic("def main() -> ():\n    x: Missing = 3\n"),
        "2:8: unknown type `Missing`"
    );
}

#[test]
fn python_builtins_name_the_plenty_spelling() {
    // Issue #573.
    for (call, name, hint) in [
        ("abs(-1)", "abs", "import `std.math` and call `math.abs(x)`"),
        (
            "min(1, 2)",
            "min",
            "import `std.math` and call `math.min(a, b)`, or `math.min_all(&items)` for a list",
        ),
        (
            "max(1, 2)",
            "max",
            "import `std.math` and call `math.max(a, b)`, or `math.max_all(&items)` for a list",
        ),
        (
            "sum(1)",
            "sum",
            "import `std.math` and call `math.sum(&items, 0)`",
        ),
        (
            "round(2.5)",
            "round",
            "import `std.math` and call `math.round(x)?`, or keep the float with `x.round()`",
        ),
        (
            "int(\"3\")",
            "int",
            "convert a number with `i64(value)`, or parse text with `i64.parse(text)?`",
        ),
        (
            "float(\"3\")",
            "float",
            "convert a number with `f64(value)`, or parse text with `f64.parse(text)?`",
        ),
    ] {
        assert_eq!(
            diagnostic(&format!("def main() -> ():\n    y = {call}\n")),
            format!("2:9: unknown function `{name}`; {hint}"),
        );
    }
    assert_eq!(
        diagnostic("def main() -> ():\n    y = missing(1)\n"),
        "2:9: unknown function `missing`"
    );
    assert_eq!(
        output(
            "import std.math as math\ndef main() -> Result[(), AllocError | IoError | ParseError | math.MathError]:\n    xs = [3, 1]?\n    print(math.max(1, 2), math.max_all(&xs), math.sum(&xs, 0), math.round(2.5)?, i64.parse(\"3\")?)?\n    Ok(())\n"
        ),
        "2 Option[i64].Some(3) 4 2 3\n"
    );
}

#[test]
fn iterating_a_file_names_the_line_readers() {
    // Issue #574.
    let expected = "a file is not iterable; read lines with `line = file.readline()?` in a `while` loop until it returns `\"\"`, or collect them with `file.readlines()?`";
    for (iterable, column) in [("file", 21), ("&file", 21), ("&mut file", 21)] {
        let source = format!(
            "def main() -> Result[(), IoError]:\n    with open(\"lines.txt\")? as file:\n        for line in {iterable}:\n            pass\n    Ok(())\n"
        );
        assert_eq!(
            diagnostic(&source),
            format!("3:{column}: {expected}"),
            "{iterable}"
        );
    }
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), IoError]:\n    for line in open(\"lines.txt\")?:\n        pass\n    Ok(())\n"
        ),
        format!("2:34: {expected}")
    );
}

#[test]
fn a_member_variant_pattern_on_a_sum_names_the_member_pattern() {
    // Issue #575.
    let declarations = "enum A:\n    X\n    Y\nenum B:\n    Z\n";
    let hint = "a sum pattern names a whole member: write `case value: A:` in a `match` on the sum, then match `A` variants on `value` in a nested `match`";
    assert_eq!(
        diagnostic(&format!(
            "{declarations}def f() -> A | B:\n    A.X\ndef main() -> ():\n    match f():\n        case A.X:\n            pass\n        case _:\n            pass\n"
        )),
        format!("10:14: expected A | B, got A; {hint}")
    );
    assert_eq!(
        diagnostic(&format!(
            "{declarations}def f() -> Result[i64, A | B]:\n    Err(A.X)\ndef main() -> ():\n    match f():\n        case Ok(v):\n            pass\n        case Err(A.X):\n            pass\n        case Err(_):\n            pass\n"
        )),
        format!("12:18: expected A | B, got A; {hint}")
    );
    // A builtin error member.
    assert_eq!(
        diagnostic(
            "def f() -> Result[i64, AllocError | ParseError]:\n    Ok(1)\ndef main() -> ():\n    match f():\n        case Ok(v):\n            pass\n        case Err(ParseError.Invalid):\n            pass\n        case Err(_):\n            pass\n"
        ),
        "7:18: expected AllocError | ParseError, got ParseError; a sum pattern names a whole member: write `case value: ParseError:` in a `match` on the sum, then match `ParseError` variants on `value` in a nested `match`"
    );
    // An enum that is not a member keeps the plain mismatch.
    assert_eq!(
        diagnostic(&format!(
            "{declarations}enum C:\n    W\ndef f() -> A | B:\n    A.X\ndef main() -> ():\n    match f():\n        case C.W:\n            pass\n        case _:\n            pass\n"
        )),
        "12:14: expected A | B, got C"
    );
    assert_eq!(
        output(&format!(
            "{declarations}def f() -> A | B:\n    A.Y\ndef main() -> Result[(), IoError]:\n    match f():\n        case value: A:\n            match value:\n                case A.X:\n                    print(\"x\")?\n                case A.Y:\n                    print(\"y\")?\n        case _:\n            pass\n    Ok(())\n"
        )),
        "y\n"
    );
}

#[test]
fn moving_out_through_a_reference_leads_with_borrowing() {
    // Issue #576: borrow and `replace` come before a fallible copy, which
    // propagates with `?` rather than `unwrap`.
    let class = "class Bag:\n    items: list[i64]\n";
    assert_eq!(
        diagnostic(&format!(
            "{class}def take(b: &mut Bag) -> list[i64]:\n    b.items\ndef main() -> ():\n    pass\n"
        )),
        "4:5: cannot move out of a field through a reference; borrow it with &value.field (or &mut value.field), take it with replace(value.field, replacement) through a mutable reference, or copy it with copy(value.field)?"
    );
    assert_eq!(
        diagnostic(&format!(
            "{class}def main() -> Result[(), AllocError]:\n    bags = [Bag([1]?)]?\n    y = bags[0].items\n    Ok(())\n"
        )),
        "5:9: cannot move out of a field here: fields move out only from a named class binding, not through a Box, Shared handle, element, capture, or temporary; borrow it with &value.field (or &mut value.field), take it with replace(value.field, replacement) on a mutable owner, or copy it with copy(value.field)?"
    );
    assert_eq!(
        diagnostic(
            "def main() -> Result[(), AllocError]:\n    rows = [[1]?]?\n    y = rows[0]\n    Ok(())\n"
        ),
        "3:9: cannot move out of indexed storage; borrow it with &collection[index], take it with replace(collection[index], value) or collection.pop(index), or copy it with copy(collection[index])?"
    );
    assert_eq!(
        diagnostic(
            "def take(xs: &list[i64]) -> list[i64]:\n    *xs\ndef main() -> ():\n    pass\n"
        ),
        "2:5: cannot move out of a reference; use the reference instead of dereferencing it, or copy the value with copy(reference)?"
    );
    assert_eq!(
        output(&format!(
            "{class}def take(b: &mut Bag) -> Result[list[i64], AllocError]:\n    copy(b.items)\ndef swap(b: &mut Bag) -> Result[list[i64], AllocError]:\n    Ok(replace(b.items, [9]?))\ndef main() -> Result[(), AllocError | IoError]:\n    mut bag = Bag([1]?)\n    print(take(&mut bag)?)?\n    old = swap(&mut bag)?\n    print(old, bag.items)?\n    Ok(())\n"
        )),
        "[1]\n[1] [9]\n"
    );
}

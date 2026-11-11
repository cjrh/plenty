//! Integer intervals (issue #952): the checker derives an inclusive range for
//! an integer expression by the rules in
//! `book/src/design/language/03-types/intervals.md`. An operation whose range
//! fits its type adds no error and has no run-time check, and
//! `Bounded.from` accepts a value whose range lies inside its bounds.
use std::path::Path;
use std::process::Command;

mod support;
use support::{assert_stdout as runs, check_diagnostic, check_source};

/// (type, bits, signed)
const WIDTHS: [(&str, u32, bool); 8] = [
    ("i8", 8, true),
    ("i16", 16, true),
    ("i32", 32, true),
    ("i64", 64, true),
    ("u8", 8, false),
    ("u16", 16, false),
    ("u32", 32, false),
    ("u64", 64, false),
];

fn limits(bits: u32, signed: bool) -> (i128, i128) {
    if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

fn accepted(source: &str) {
    if let Err(error) = check_source(source) {
        panic!("{source}\n{error}");
    }
}

fn rejected(source: &str, message: &str) {
    let error = check_diagnostic(source);
    assert!(error.contains(message), "{source}\n{error}");
}

/// A function of `a: ty` returning plain `ty`, so any error its body could
/// return is a compile error.
fn infallible(ty: &str, body: &str) -> String {
    format!("def f(a: {ty}) -> {ty}:\n    {body}\ndef main() -> ():\n    pass\n")
}

#[test]
fn arithmetic_at_each_width_edge_is_infallible_exactly_when_it_fits() {
    for (ty, bits, signed) in WIDTHS {
        let count = bits - 1;
        let mut fits = vec![
            // 0..9 added below the maximum, and subtracted above the minimum.
            format!("a % 10 + ({ty}.MAX - 9)"),
            format!("({ty}.MIN + 9) - a % 10"),
            // 1..2 times half the maximum.
            format!("(a % 2 + 1) * ({ty}.MAX // 2)"),
            // A divisor in 1..3 is never zero or -1.
            "a // (a % 3 + 1)".to_string(),
            "a % (a % 3 + 1)".to_string(),
            // A count in 0..bits-1.
            format!("a >> (a & {count})"),
            format!("a << (a % {bits})"),
        ];
        let mut fails = vec![
            (
                format!("a % 10 + ({ty}.MAX - 8)"),
                format!("OverflowError[{ty}]"),
            ),
            (
                format!("({ty}.MIN + 8) - a % 10"),
                format!("OverflowError[{ty}]"),
            ),
            (
                format!("(a % 2 + 1) * ({ty}.MAX // 2 + 1)"),
                format!("OverflowError[{ty}]"),
            ),
            ("a // (a % 3)".to_string(), "ZeroDivisionError".to_string()),
            ("a % (a % 3)".to_string(), "ZeroDivisionError".to_string()),
            (
                format!("a << (a % {})", bits + 1),
                format!("ShiftCountError[{ty}]"),
            ),
            (
                format!("a >> (a % {})", bits + 1),
                format!("ShiftCountError[{ty}]"),
            ),
        ];
        if signed {
            // MIN + 1..MIN + 2 negates to MAX - 1..MAX; MIN itself does not.
            fits.push(format!("-({ty}.MIN + 1 + a % 2)"));
            fits.push(format!("(a % 2 + 1) * ({ty}.MIN // 2)"));
            // -3..-1 can be -1, but -2..2 cannot be MIN.
            fits.push("(a % 5 - 2) // -(a % 3 + 1)".to_string());
            fits.push("a % -(a % 3 + 1)".to_string());
            // -9..0 is no shift count.
            fails.push((
                "a << (a % -10)".to_string(),
                format!("ShiftCountError[{ty}]"),
            ));
            fails.push((
                format!("-({ty}.MIN + a % 2)"),
                format!("OverflowError[{ty}]"),
            ));
            fails.push((
                "a // -(a % 3 + 1)".to_string(),
                format!("OverflowError[{ty}]"),
            ));
            // -1..1 includes zero.
            fails.push((
                "a // (a % 3 - 1)".to_string(),
                "ZeroDivisionError".to_string(),
            ));
        }
        for body in fits {
            accepted(&infallible(ty, &body));
        }
        for (body, error) in fails {
            rejected(
                &infallible(ty, &body),
                &format!("arithmetic can fail with {error}"),
            );
        }
    }
}

#[test]
fn results_at_the_interval_extremes_are_exact() {
    for (ty, bits, signed) in WIDTHS {
        let (min, max) = limits(bits, signed);
        let mut source = format!(
            "def top(a: {ty}) -> {ty}:
    a % 10 + ({ty}.MAX - 9)
def bottom(a: {ty}) -> {ty}:
    ({ty}.MIN + 9) - a % 10
def product(a: {ty}) -> {ty}:
    (a % 2 + 1) * ({ty}.MAX // 2)
def quotient(a: {ty}) -> {ty}:
    a // (a % 3 + 1)
"
        );
        let mut calls = vec![
            "top(9)",
            "top(10)",
            "bottom(9)",
            "product(1)",
            "quotient(5)",
        ];
        let mut expected = vec![max, max - 9, min, max - 1, 1];
        if signed {
            source += &format!(
                "def negated(a: {ty}) -> {ty}:
    -({ty}.MIN + 1 + a % 2)
def lowest(a: {ty}) -> {ty}:
    (a % 2 + 1) * ({ty}.MIN // 2)
def floored(a: {ty}) -> {ty}:
    (a % 5 - 2) // -(a % 3 + 1)
def remainder(a: {ty}) -> {ty}:
    a % -(a % 3 + 1)
"
            );
            calls.extend([
                "negated(0)",
                "lowest(1)",
                "floored(4)",
                "floored(0)",
                "remainder(7)",
                "remainder(-7)",
            ]);
            // 4: 2 // -2 = -1. 0: -2 // -1 = 2. 7: 7 % -2 = -1. -7: -7 % -3 = -1.
            expected.extend([max, min, -1, 2, -1, -1]);
        }
        source += &format!(
            "def main() -> Result[(), IoError]:\n    print({})?\n    Ok(())\n",
            calls.join(", ")
        );
        let expected: Vec<String> = expected.iter().map(i128::to_string).collect();
        runs(&source, &format!("{}\n", expected.join(" ")));
    }
}

/// `Bounded.from` accepts exactly the interval `lo..hi` for `expression`: the
/// same bounds are accepted, and each bound moved inward by one is rejected
/// with the interval named.
fn interval_is(expression: &str, lo: i128, hi: i128) {
    let program = |lo: i128, hi: i128| {
        format!(
            "const SEVEN: i64 = 7
def f(x: i64, u: u8, s: Bounded[i64, -5, 7], n: NonNegative[i64], p: Positive[i64], xs: &list[i64]) -> Bounded[i64, {lo}, {hi}]:
    Bounded[i64, {lo}, {hi}].from({expression})
def main() -> ():
    pass
"
        )
    };
    accepted(&program(lo, hi));
    let named = format!("this value is in {lo}..{hi}, which is not inside");
    if lo < hi {
        rejected(&program(lo + 1, hi), &named);
        rejected(&program(lo, hi - 1), &named);
    } else if lo > i128::from(i64::MIN) {
        rejected(&program(lo - 1, hi - 1), &named);
    }
}

#[test]
fn each_rule_gives_its_documented_interval() {
    let max = i128::from(i64::MAX);
    let min = i128::from(i64::MIN);
    for (expression, lo, hi) in [
        // Sources.
        ("5", 5, 5),
        ("SEVEN", 7, 7),
        ("i64.MAX", max, max),
        ("s.get()", -5, 7),
        ("n.get()", 0, max),
        ("p.get()", 1, max),
        ("len(xs)", 0, max),
        ("(s.get())", -5, 7),
        ("x", min, max),
        // Addition, subtraction, multiplication, negation.
        ("s.get() + 1", -4, 8),
        ("s.get() - 10", -15, -3),
        ("s.get() - s.get()", -12, 12),
        ("s.get() * -2", -14, 10),
        ("s.get() * s.get()", -35, 49),
        ("-s.get()", -7, 5),
        ("i64.MAX - x % 10", max - 9, max),
        ("len(xs) - 1", -1, max - 1),
        // Floored division.
        ("s.get() // 2", -3, 3),
        ("s.get() // -2", -4, 2),
        ("x // 2", min / 2, max / 2),
        ("(x % 7 - 3) // (x % 7 + 1)", -3, 3),
        ("len(xs) // 2", 0, max / 2),
        // The remainder has the divisor's sign.
        ("x % 10", 0, 9),
        ("x % SEVEN", 0, 6),
        ("x % -10", -9, 0),
        ("s.get() % 4", 0, 3),
        ("(x % 3) % 10", 0, 2),
        ("(x % -3) % -10", -2, 0),
        ("x % p.get()", 0, max - 1),
        // Masks.
        ("x & 15", 0, 15),
        ("15 & x", 0, 15),
        ("s.get() & 12", 0, 12),
        ("(x & 15) & (x & 3)", 0, 3),
        ("x & -1", min, max),
        // Shifts by a count in range.
        ("s.get() >> 1", -3, 3),
        ("(x & 3) << 2", 0, 12),
        ("x >> 60", -8, 7),
        ("x << 1", min, max),
        // Casts.
        ("i64(u)", 0, 255),
        ("i64(u) - 128", -128, 127),
        ("i64(i8(x & 100))", 0, 100),
        ("x * 0 + x", min, max),
        // No rule.
        ("x | 1", min, max),
        ("x ^ 1", min, max),
        ("x.wrapping_add(1) & -1", min, max),
    ] {
        interval_is(expression, lo, hi);
    }
}

#[test]
fn only_immutable_bindings_carry_an_interval() {
    accepted(
        "def f(x: i64) -> i64:
    last = x % 10
    scaled: i64 = last * 100
    scaled * scaled
def main() -> ():
    pass
",
    );
    for body in [
        "mut last = x % 10\n    last * 2",
        "mut last = x % 10\n    last = x\n    last * 2",
        // A loop variable, a pattern binding and a field have the full range.
        "for i in range(3):\n        return i * 2\n    0",
        "pair = (x % 10, 1)\n    (first, _) = pair\n    first * 2",
    ] {
        rejected(
            &format!("def f(x: i64) -> i64:\n    {body}\ndef main() -> ():\n    pass\n"),
            "arithmetic can fail with OverflowError[i64]",
        );
    }
    // A parameter keeps the full range after a comparison.
    rejected(
        "def f(x: i64) -> i64:
    if x >= 0 and x < 10:
        return x * 2
    0
def main() -> ():
    pass
",
        "arithmetic can fail with OverflowError[i64]",
    );
    // Each branch's binding has its own interval.
    runs(
        "def f(flag: bool, x: i64) -> i64:
    if flag:
        k = x % 10
        return k * 900000000000000000
    else:
        k = x % 1000
        return k * 9000000000000000
def main() -> Result[(), IoError]:
    print(f(True, 9), f(False, 999))?
    Ok(())
",
        "8100000000000000000 8991000000000000000\n",
    );
    rejected(
        "def f(flag: bool, x: i64) -> i64:
    if flag:
        k = x % 10
        return k * 900000000000000000
    else:
        k = x % 1000
        return k * 900000000000000000
def main() -> ():
    pass
",
        "7:16: arithmetic can fail with OverflowError[i64]",
    );
}

#[test]
fn a_value_that_could_have_failed_has_the_full_range() {
    // `unchecked:` assumes the addition does not overflow; it does not make
    // its result an interval.
    rejected(
        "def f(s: Bounded[i64, 0, 9]) -> Bounded[i64, 0, 10]:
    unchecked:
        next = s.get() + i64.MAX - i64.MAX + 1
        Bounded[i64, 0, 10].from(next)
def main() -> ():
    pass
",
        "this value is in -9223372036854775808..9223372036854775807",
    );
    // A class's `len` method returns any i64.
    rejected(
        "class Odd:
    n: i64
    def len(self) -> i64:
        self.n
def f(o: &Odd) -> i64:
    len(o) - 1
def main() -> ():
    pass
",
        "arithmetic can fail with OverflowError[i64]",
    );
    // An operator method proves nothing about its result.
    rejected(
        "@copy
class Wild:
    n: i64
    def __mod__(self, other: i64) -> i64:
        self.n
def f(w: Wild) -> i64:
    w % 10 + 1
def main() -> ():
    pass
",
        "arithmetic can fail with OverflowError[i64]",
    );
    // A call's result has the full range, whatever the callee returns.
    rejected(
        "def digit(x: i64) -> i64:
    x % 10
def f(x: i64) -> i64:
    digit(x) + 1
def main() -> ():
    pass
",
        "arithmetic can fail with OverflowError[i64]",
    );
}

/// An interval source is decided by the type or callee the expression
/// resolved to. A source nested in its receiver or argument, or a
/// declaration sharing a source's name, changes nothing.
#[test]
fn a_source_is_decided_by_the_resolved_type_or_callee() {
    const OVERFLOW: &str = "arithmetic can fail with OverflowError[i64]";
    const FULL: &str = "this value is in -9223372036854775808..9223372036854775807";
    let classes = "class Neg:
    count: i64
    def len(self) -> i64:
        self.count
    def get(self) -> i64:
        self.count
    def pick(self, k: i64) -> i64:
        self.count
enum Kind:
    A
    B
    def len(self) -> i64:
        -7
protocol Measured:
    def len(self) -> i64:
        pass
def pick(ws: &list[Neg], i: i64) -> &Neg:
    &ws[i]
";
    let program = |function: &str| format!("{classes}{function}\ndef main() -> ():\n    pass\n");
    for (function, message) in [
        // A class or enum `len` whose receiver contains a built-in `len`.
        (
            "def f(ws: &list[Neg], xs: &list[i64]) -> i64:\n    n = len(ws[len(xs) - 1])\n    n - i64.MAX",
            OVERFLOW,
        ),
        (
            "def f(ws: &list[Neg], xs: &list[i64]) -> i64:\n    len(pick(ws, len(xs) - 1)) - i64.MAX",
            OVERFLOW,
        ),
        (
            "def f(ks: &list[Kind], xs: &list[i64]) -> i64:\n    len(ks[len(xs) - 1]) - i64.MAX",
            OVERFLOW,
        ),
        (
            "def f(ws: &list[Neg], xs: &list[i64]) -> Bounded[i64, 0, 9223372036854775807]:\n    Bounded[i64, 0, 9223372036854775807].from(len(ws[len(xs) - 1]))",
            FULL,
        ),
        // The method called by name.
        ("def f(w: &Neg) -> i64:\n    w.len() - i64.MAX", OVERFLOW),
        // A class `get`, also with a domain `get` in its receiver or beside
        // it as an argument.
        (
            "def f(w: &Neg) -> Bounded[i64, 0, 3]:\n    Bounded[i64, 0, 3].from(w.get())",
            FULL,
        ),
        (
            "def f(ws: &list[Neg], b: Bounded[i64, 0, 3]) -> Bounded[i64, 0, 3]:\n    Bounded[i64, 0, 3].from(ws[b.get()].get())",
            FULL,
        ),
        (
            "def f(w: &Neg, b: Bounded[i64, 0, 3]) -> Bounded[i64, 0, 3]:\n    Bounded[i64, 0, 3].from(w.pick(b.get()))",
            FULL,
        ),
    ] {
        rejected(&program(function), message);
    }
    // A protocol's `len`, in the instantiation that calls it.
    rejected(
        &format!(
            "{classes}def f[M: Measured](ws: &list[M], xs: &list[i64]) -> i64:
    len(ws[len(xs) - 1]) - i64.MAX
def main() -> Result[(), AllocError | IoError]:
    ws = [Neg(-5)]?
    xs = [1]?
    print(f(&ws, &xs))?
    Ok(())
"
        ),
        OVERFLOW,
    );
    for function in [
        // A built-in `len` whose argument contains a class `len`.
        "def f(w: &Neg, xs: &list[list[i64]]) -> i64:\n    len(xs[len(w)]) - 1",
        // A domain `get` whose receiver contains a domain `get` and a
        // built-in `len`.
        "def f(bs: &list[Bounded[i64, 0, 3]], b: Bounded[i64, 0, 3]) -> i64:\n    bs[b.get()].get() + (i64.MAX - 3)",
        "def f(bs: &list[Bounded[i64, 0, 3]]) -> i64:\n    bs[len(bs) - 1].get() + (i64.MAX - 3)",
    ] {
        accepted(&program(function));
    }

    // A parameter, local or value parameter named like a module constant is
    // not that constant.
    for function in [
        "def f(N: i64) -> i64:\n    N + (i64.MAX - 5)",
        "def f(a: i64) -> i64:\n    N = a\n    N + (i64.MAX - 5)",
    ] {
        rejected(
            &format!("const N: i64 = 5\n{function}\ndef main() -> ():\n    pass\n"),
            OVERFLOW,
        );
    }
    rejected(
        "const N: i64 = 5
def f[const N: i64](a: i64) -> i64:
    N + (i64.MAX - 5)
def main() -> Result[(), IoError]:
    print(f[100](1))?
    Ok(())
",
        OVERFLOW,
    );
    accepted(
        "const N: i64 = 5\ndef f() -> i64:\n    N + (i64.MAX - 5)\ndef main() -> ():\n    pass\n",
    );
    // A local named like an integer type is a value, not the type.
    rejected(
        "def f(a: i64) -> i64:\n    i8 = a\n    i8 + 1\ndef main() -> ():\n    pass\n",
        OVERFLOW,
    );
    // The names of the sources cannot be redefined.
    for (declaration, name) in [
        ("def len(xs: &list[i64]) -> i64:\n    -5", "len"),
        ("def i32(x: i64) -> i64:\n    x", "i32"),
        ("class u8:\n    n: i64", "u8"),
        ("class Bounded:\n    n: i64", "Bounded"),
    ] {
        rejected(
            &format!("{declaration}\ndef main() -> ():\n    pass\n"),
            &format!("cannot redefine builtin `{name}`"),
        );
    }

    // `T.MIN` and `T.MAX` follow the type `T` resolves to: an alias, and a
    // type parameter per instantiation.
    accepted(
        "type Tiny = i8
type Index = Bounded[i64, 0, 9]
def f(a: Tiny) -> Tiny:
    a % 10 + (Tiny.MAX - 9)
def g(a: i64, i: Index) -> i64:
    j = Index.from(a % 10)
    j.get() + i.get() + (i64.MAX - 18)
def main() -> ():
    pass
",
    );
    rejected(
        "type Tiny = i8\ndef f(a: Tiny) -> Tiny:\n    a % 10 + (Tiny.MAX - 8)\ndef main() -> ():\n    pass\n",
        "arithmetic can fail with OverflowError[i8]",
    );
    let generic = |slack: u8, call: &str| {
        format!(
            "def f[T: IntType](a: T) -> T:
    a % T(10) + (T.MAX - T({slack}))
def main() -> Result[(), IoError]:
    print({call})?
    Ok(())
"
        )
    };
    runs(&generic(9, "f(i8(5)), f(u16(7))"), "123 65533\n");
    rejected(
        &generic(8, "f(i8(5))"),
        "arithmetic can fail with OverflowError[i8]",
    );
    rejected(
        &generic(8, "f(u16(7))"),
        "arithmetic can fail with OverflowError[u16]",
    );
}

/// The run-time effect of the class `len` case: no silent wrap, and a
/// negative index still counts from the end.
#[test]
fn a_class_length_keeps_its_checks_at_run_time() {
    runs(
        "class Neg:
    count: i64
    def len(self) -> i64:
        self.count
def f(ws: &list[Neg], xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    n = len(ws[len(xs) - 1])
    Ok(n - i64.MAX)
def g(ws: &list[Neg], xs: &list[i64], items: &list[i64]) -> Result[i64, OverflowError[i64]]:
    n = len(ws[len(xs) - 1])
    Ok(items[n + 0])
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    ws = [Neg(-5)]?
    last = [Neg(-1)]?
    xs = [1]?
    items = [10, 20, 30]?
    print(f(&ws, &xs).is_err(), g(&last, &xs, &items)?)?
    Ok(())
",
        "True 30\n",
    );
}

#[test]
fn bounded_from_names_the_interval_it_rejects() {
    let program = |body: &str| {
        format!(
            "type Digit = Bounded[i64, 0, 9]
def f(x: i64, d: Digit) -> Digit:
    {body}
def main() -> ():
    pass
"
        )
    };
    for body in [
        "Digit.from(x % 10)",
        "Digit.from(d.get() // 2 + 5)",
        "Digit.from((d.get() * 3877 + 29573) % 10)",
        "Digit.from(9 - d.get())",
        "next = d.get() % 9 + 1\n    Digit.from(next)",
    ] {
        accepted(&program(body));
    }
    for (body, message) in [
        (
            "Digit.from(d.get() + 1)",
            "3:16: this value is in 1..10, which is not inside `Bounded[i64, 0, 9]`'s inclusive bounds 0..9; use `Bounded.try_from(value)?` to check it at run time",
        ),
        ("Digit.from(d.get() - 1)", "this value is in -1..8"),
        ("Digit.from(x % 11)", "this value is in 0..10"),
        ("Digit.from(-d.get())", "this value is in -9..0"),
        (
            "Digit.from(x)",
            "this value is in -9223372036854775808..9223372036854775807",
        ),
        (
            "mut next = x % 10\n    Digit.from(next)",
            "this value is in -9223372036854775808..9223372036854775807",
        ),
    ] {
        rejected(&program(body), message);
    }
}

#[test]
fn a_proved_function_declares_no_error_and_a_declared_one_stays_valid() {
    runs(
        "const IM: i64 = 139968
type Seed = Bounded[i64, 0, 139967]

class Rng:
    seed: Seed

    def next(self: &mut Rng) -> i64:
        self.seed = Seed.from((self.seed.get() * 3877 + 29573) % IM)
        self.seed.get()

def mean(a: u8, b: u8) -> i64:
    (i64(a) + i64(b)) // 2

# Declaring an error the body cannot return stays valid.
def declared(a: u8, b: u8) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    Ok((i64(a) + i64(b)) // 2)

def main() -> Result[(), IoError | OverflowError[i64] | ZeroDivisionError]:
    mut rng = Rng(42)
    first = rng.next()
    print(first, rng.next(), mean(255, 255), declared(255, 254)?)?
    Ok(())
",
        "52439 102040 255 254\n",
    );
    // A `checked:` block must contain arithmetic that can fail.
    rejected(
        "def f(a: u8, b: u8) -> i64:
    result = checked:
        i64(a) + i64(b)
    result.unwrap_or(0)
def main() -> ():
    pass
",
        "this `checked:` block contains no arithmetic that can fail",
    );
    // One that keeps a fallible operation reports only that operation's error.
    runs(
        "def f(a: i64, b: i64) -> i64:
    result = checked:
        (a % 10 + 1) * b
    match result:
        case Ok(value):
            value
        case Err(OverflowError(wrapped)):
            wrapped
def main() -> Result[(), IoError]:
    print(f(19, 3), f(1, i64.MAX))?
    Ok(())
",
        "30 -2\n",
    );
}

/// The `--release` CLIF of function `name` in `source`.
fn clif(source: &str, name: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = Command::new(support::plenty_bin())
        .current_dir::<&Path>(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--release",
            "--dump",
            "clif-opt",
            "main.plenty",
            "-o",
            "out",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = std::fs::read_to_string(dir.path().join("out/clif-opt.txt")).unwrap();
    let start = dump
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("no section {name} in:\n{dump}"));
    let rest = &dump[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

/// Overflow tests and conditional branches in a function's CLIF.
fn checks(body: &str) -> (usize, usize) {
    let overflow = ["sadd_overflow", "ssub_overflow", "smul_overflow"]
        .iter()
        .map(|name| body.matches(name).count())
        .sum();
    (overflow, body.matches("brif").count())
}

// The checker's verdict selects the operation code generation emits, so an
// operation with no declared error has no check, and the same operation on
// operands the checker cannot bound keeps it.
#[test]
fn generated_code_has_a_check_exactly_where_the_checker_requires_an_error() {
    let source = "\
type Small = Bounded[i64, -1000, 1000]
type Checked = Result[i64, OverflowError[i64] | ZeroDivisionError]

def proved(a: Small, b: Small) -> i64:
    (a.get() * b.get() + 7 - b.get()) // (a.get() % 5 + 1)

def unproved(a: i64, b: i64) -> Checked:
    Ok((a * b + 7 - b) // (a % 5))

def partly(a: Small, b: i64) -> Checked:
    Ok(a.get() * 1000 + b)

def main() -> Result[(), IoError | OverflowError[i64] | ZeroDivisionError]:
    print(proved(1000, -1000), unproved(3, 4)?, partly(-1000, 5)?)?
    print(unproved(5, 1).is_err(), unproved(i64.MAX, 2).is_err(), partly(1, i64.MAX).is_err())?
    Ok(())
";
    runs(source, "-998993 5 -999995\nTrue True True\n");
    assert_eq!(checks(&clif(source, "proved")), (0, 0));
    // Multiply, add, subtract, and the zero divisor.
    let (overflow, branches) = checks(&clif(source, "unproved"));
    assert_eq!(overflow, 3);
    assert!(branches >= 4, "{branches}");
    // Only the addition of the unbounded operand.
    assert_eq!(checks(&clif(source, "partly")).0, 1);
}

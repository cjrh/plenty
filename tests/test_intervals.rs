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

/// Compiled programs agree with `plenty::interval::exact`, the reference
/// the interval rules are tested against: every pair of values of the 8-bit
/// types, and the values around zero, the width and both limits of the
/// wider ones.
#[test]
fn compiled_operators_match_the_reference_semantics() {
    use plenty::interval::{Exact, Operator, exact};
    const OPERATORS: [(&str, Operator); 8] = [
        ("+", Operator::Add),
        ("-", Operator::Sub),
        ("*", Operator::Mul),
        ("//", Operator::FloorDiv),
        ("%", Operator::Modulo),
        ("&", Operator::And),
        ("<<", Operator::ShiftLeft),
        (">>", Operator::ShiftRight),
    ];
    let mut source = String::new();
    let mut main = String::new();
    let mut expected = String::new();
    for (ty, bits, signed) in WIDTHS {
        let (min, max) = limits(bits, signed);
        let error = format!("OverflowError[{ty}] | ZeroDivisionError | ShiftCountError[{ty}]");
        for (index, (symbol, _)) in OPERATORS.iter().enumerate() {
            source += &format!(
                "def op{index}_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, {error}]:\n    Ok(a {symbol} b)\n"
            );
        }
        let values: Vec<i128> = if bits == 8 {
            (min..=max).collect()
        } else {
            let width = i128::from(bits);
            let mut values = vec![min, min + 1, max - 1, max, max / 2, max / 2 + 1];
            for near in [0, width] {
                values.extend((near - 2..=near + 2).filter(|value| (min..=max).contains(value)));
            }
            if signed {
                values.extend([-width - 1, -width, -width + 1, min / 2]);
            }
            values.sort_unstable();
            values.dedup();
            values
        };
        let literal = |value: i128| {
            if value == min {
                format!("{ty}.MIN")
            } else if value < 0 {
                format!("{ty}.MIN + {ty}({})", value - min)
            } else {
                format!("{value}{ty}")
            }
        };
        let list: Vec<String> = values.iter().map(|&value| literal(value)).collect();
        // One line per left operand and operator: a count of each failure
        // and a sum of the results weighted by the right operand's place,
        // in wrapping `u64` arithmetic.
        source += &format!(
            "def run_{ty}() -> Result[(), AllocError | IoError]:
    xs: list[{ty}] = [{list}]?
    for i in range(len(xs)):
        a = xs[i]
",
            list = list.join(", ")
        );
        for index in 0..OPERATORS.len() {
            source += &format!(
                "        mut sum{index} = 0u64
        mut overflow{index} = 0u64
        mut zero{index} = 0u64
        mut count{index} = 0u64
        mut weight{index} = 0u64
        for j in range(len(xs)):
            weight{index} = weight{index}.wrapping_add(1u64)
            weight = weight{index}
            match op{index}_{ty}(a, xs[j]):
                case Ok(v):
                    sum{index} = sum{index}.wrapping_add(u64.wrapping_from(v).wrapping_mul(weight))
                case Err(OverflowError(_)):
                    overflow{index} = overflow{index}.wrapping_add(weight)
                case Err(ZeroDivisionError()):
                    zero{index} = zero{index}.wrapping_add(weight)
                case Err(ShiftCountError(_)):
                    count{index} = count{index}.wrapping_add(weight)
        print(sum{index}, overflow{index}, zero{index}, count{index})?
"
            );
        }
        for &a in &values {
            for (_, operator) in OPERATORS {
                let (mut sum, mut overflow, mut zero, mut count) = (0u64, 0u64, 0u64, 0u64);
                for (place, &b) in values.iter().enumerate() {
                    let weight = place as u64 + 1;
                    match exact(operator, signed, bits, a, b) {
                        Exact::Value(value) => {
                            sum = sum.wrapping_add((value as u64).wrapping_mul(weight));
                        }
                        Exact::Overflow => overflow += weight,
                        Exact::ZeroDivision => zero += weight,
                        Exact::BadCount => count += weight,
                    }
                }
                expected += &format!("{sum} {overflow} {zero} {count}\n");
            }
        }
        source += "    Ok(())\n";
        main += &format!("    run_{ty}()?\n");
    }
    source += &format!("def main() -> Result[(), AllocError | IoError]:\n{main}    Ok(())\n");
    runs(&source, &expected);
}

/// The stdout of `source` compiled and run with `flags`.
fn stdout_with(source: &str, flags: &[&str]) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = Command::new(support::plenty_bin())
        .current_dir::<&Path>(dir.path())
        .env("PLENTY_CACHE", "off")
        .args(flags)
        .arg("main.plenty")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{flags:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The `--dump ops` section of each function in `source`, by name.
fn operations(source: &str) -> std::collections::HashMap<String, String> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = Command::new(support::plenty_bin())
        .current_dir::<&Path>(dir.path())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "ops", "main.plenty", "-o", "out"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = std::fs::read_to_string(dir.path().join("out/ops.txt")).unwrap();
    dump.split("==== ")
        .filter_map(|section| {
            let (head, body) = section.split_once('\n')?;
            Some((head.split(' ').next()?.to_string(), body.to_string()))
        })
        .collect()
}

/// Values of `lo..=hi` at and next to both ends, around zero and in the
/// middle.
fn edge_values(lo: i128, hi: i128) -> Vec<i128> {
    let mut values = vec![lo, lo + 1, lo + (hi - lo) / 2, hi - 1, hi, -1, 0, 1];
    values.retain(|value| (lo..=hi).contains(value));
    values.sort_unstable();
    values.dedup();
    values
}

/// Proved operations compute the reference result at the ends of their
/// operands' intervals, in default and `--release` builds. Each function
/// returns a plain integer, so the checker proved every operator in it; the
/// operation dump confirms that the operator under test was emitted as a
/// proved operation, and that its recorded interval contains every result.
/// Most cases have a result interval that reaches a limit of the type.
#[test]
fn proved_operations_are_exact_at_the_interval_bounds() {
    use plenty::interval::{Exact, Operator, exact};
    type Range = (i128, i128);
    type Reference = Box<dyn Fn(i128) -> i128>;
    struct Case {
        name: String,
        ty: &'static str,
        /// The text the operation dump must contain, or empty for an
        /// operator that needs no proof.
        marker: &'static str,
        results: Vec<i128>,
    }
    let mut source = String::new();
    let mut main = String::new();
    let mut expected = String::new();
    let mut cases: Vec<Case> = Vec::new();
    let literal = |ty: &str, min: i128, value: i128| {
        if value == min && min < 0 {
            format!("{ty}.MIN")
        } else {
            format!("{value}{ty}")
        }
    };
    // A loop nest over `values` of each operand that converts each to its
    // `Bounded` type at run time, so no operand is a compile-time constant,
    // and prints the result of `name`.
    let emit = |source: &mut String,
                main: &mut String,
                name: &str,
                operands: &[(&str, Range, i128, &[i128])]| {
        let mut depth = 1;
        let mut body = String::new();
        let mut arguments = Vec::new();
        for (index, (ty, (lo, hi), min, values)) in operands.iter().enumerate() {
            let list: Vec<String> = values.iter().map(|&v| literal(ty, *min, v)).collect();
            source.push_str(&format!(
                "type {name}_T{index} = Bounded[{ty}, {lo}, {hi}]\n"
            ));
            body.push_str(&format!(
                "{pad}for v{index} in [{}]?:\n{pad}    match {name}_T{index}.try_from(v{index}):\n{pad}        case Err(_):\n{pad}            print(\"{name} out of bounds\")?\n{pad}        case Ok(b{index}):\n",
                list.join(", "),
                pad = "    ".repeat(depth),
            ));
            arguments.push(format!("b{index}"));
            depth += 3;
        }
        source.push_str(&format!(
            "def run_{name}() -> Result[(), AllocError | IoError]:\n{body}{}print({name}({}))?\n    Ok(())\n",
            "    ".repeat(depth),
            arguments.join(", ")
        ));
        main.push_str(&format!("    run_{name}()?\n"));
    };

    for (ty, bits, signed) in WIDTHS {
        let (min, max) = limits(bits, signed);
        let full = (min, max);
        let width = i128::from(bits);
        let half = max / 2;
        let binary: Vec<(&str, Operator, &str, Range, Range)> = if signed {
            let low = min / 2;
            let root = (max as f64).sqrt() as i128;
            vec![
                ("+", Operator::Add, "Proved(Add,", (min, 0), (0, max)),
                (
                    "+",
                    Operator::Add,
                    "Proved(Add,",
                    (low, half),
                    (low, half + 1),
                ),
                (
                    "-",
                    Operator::Sub,
                    "Proved(Sub,",
                    (low, half),
                    (low, half + 1),
                ),
                ("-", Operator::Sub, "Proved(Sub,", (min, -1), (min, 0)),
                ("*", Operator::Mul, "Proved(Mul,", (-1, 1), (min + 1, max)),
                ("*", Operator::Mul, "Proved(Mul,", (1, 2), (low, half)),
                (
                    "*",
                    Operator::Mul,
                    "Proved(Mul,",
                    (-root, root),
                    (-root, root),
                ),
                ("*", Operator::Mul, "Proved(Mul,", full, (0, 1)),
                (
                    "//",
                    Operator::FloorDiv,
                    "FloorDiv(Positive)",
                    full,
                    (1, max),
                ),
                (
                    "//",
                    Operator::FloorDiv,
                    "FloorDiv(NonZeroFits)",
                    (min + 1, max),
                    (min, -1),
                ),
                (
                    "//",
                    Operator::FloorDiv,
                    "FloorDiv(BelowMinusOne)",
                    full,
                    (min, -2),
                ),
                ("%", Operator::Modulo, "Modulo(Positive)", full, (1, max)),
                ("%", Operator::Modulo, "Modulo(NonZero)", full, (min, -1)),
                ("&", Operator::And, "", full, full),
                (
                    "<<",
                    Operator::ShiftLeft,
                    "Proved(Integer(ShiftLeft),",
                    (-1, 0),
                    (0, width - 1),
                ),
                (
                    "<<",
                    Operator::ShiftLeft,
                    "Proved(Integer(ShiftLeft),",
                    (low / 2, half / 2),
                    (0, 2),
                ),
                (
                    "<<",
                    Operator::ShiftLeft,
                    "Proved(Integer(ShiftLeft),",
                    (0, 1),
                    (0, width - 2),
                ),
                (
                    ">>",
                    Operator::ShiftRight,
                    "Proved(Integer(ShiftRight),",
                    full,
                    (0, width - 1),
                ),
            ]
        } else {
            let root = 1i128 << (bits / 2);
            vec![
                ("+", Operator::Add, "Proved(Add,", (0, half), (0, half + 1)),
                ("-", Operator::Sub, "Proved(Sub,", (half, max), (0, half)),
                (
                    "*",
                    Operator::Mul,
                    "Proved(Mul,",
                    (0, root - 1),
                    (0, root + 1),
                ),
                ("*", Operator::Mul, "Proved(Mul,", (0, 1), full),
                (
                    "//",
                    Operator::FloorDiv,
                    "FloorDiv(Positive)",
                    full,
                    (1, max),
                ),
                ("%", Operator::Modulo, "Modulo(Positive)", full, (1, max)),
                ("&", Operator::And, "", full, full),
                (
                    "<<",
                    Operator::ShiftLeft,
                    "Proved(Integer(ShiftLeft),",
                    (0, 1),
                    (0, width - 1),
                ),
                (
                    "<<",
                    Operator::ShiftLeft,
                    "Proved(Integer(ShiftLeft),",
                    (0, max >> 3),
                    (0, 3),
                ),
                (
                    ">>",
                    Operator::ShiftRight,
                    "Proved(Integer(ShiftRight),",
                    full,
                    (0, width - 1),
                ),
            ]
        };
        for (index, (symbol, operator, marker, a, b)) in binary.into_iter().enumerate() {
            let name = format!("binary{index}_{ty}");
            source += &format!(
                "def {name}(a: {name}_T0, b: {name}_T1) -> {ty}:\n    a.get() {symbol} b.get()\n"
            );
            let (left, right) = (edge_values(a.0, a.1), edge_values(b.0, b.1));
            let mut results = Vec::new();
            for &x in &left {
                for &y in &right {
                    let Exact::Value(value) = exact(operator, signed, bits, x, y) else {
                        panic!("{ty}: {x} {symbol} {y} has no value");
                    };
                    results.push(value);
                }
            }
            emit(
                &mut source,
                &mut main,
                &name,
                &[(ty, a, min, &left), (ty, b, min, &right)],
            );
            cases.push(Case {
                name,
                ty,
                marker,
                results,
            });
        }
        if signed {
            let name = format!("negate_{ty}");
            source += &format!("def {name}(a: {name}_T0) -> {ty}:\n    -a.get()\n");
            let values = edge_values(min + 1, max);
            emit(
                &mut source,
                &mut main,
                &name,
                &[(ty, (min + 1, max), min, &values)],
            );
            let results = values.iter().map(|value| -value).collect();
            cases.push(Case {
                name,
                ty,
                marker: "Proved(Sub,",
                results,
            });
        }
        // Operands bounded by a remainder or a mask of an unbounded value.
        let mut any = edge_values(min, max);
        any.extend(6..=10);
        let derived: [(&str, String, &str, Reference); 4] = [
            (
                "mask_add",
                format!("(a & 7) + ({ty}.MAX - 7)"),
                "Proved(Add,",
                Box::new(move |a| (a & 7) + max - 7),
            ),
            (
                "remainder_mul",
                format!("(a % 2) * {ty}.MAX"),
                "Proved(Mul,",
                Box::new(move |a| a.rem_euclid(2) * max),
            ),
            (
                "remainder_sub",
                format!("({ty}.MIN + 9) - a % 10"),
                "Proved(Sub,",
                Box::new(move |a| min + 9 - a.rem_euclid(10)),
            ),
            (
                "remainder_div",
                "(a % 7) // (a % 3 + 1)".to_string(),
                "FloorDiv(Positive)",
                Box::new(|a| a.rem_euclid(7) / (a.rem_euclid(3) + 1)),
            ),
        ];
        for (kind, body, marker, reference) in derived {
            let name = format!("{kind}_{ty}");
            source += &format!("def {name}(a: {ty}) -> {ty}:\n    {body}\n");
            let list: Vec<String> = any.iter().map(|&v| literal(ty, min, v)).collect();
            source += &format!(
                "def run_{name}() -> Result[(), AllocError | IoError]:\n    for v in [{}]?:\n        print({name}(v))?\n    Ok(())\n",
                list.join(", ")
            );
            main += &format!("    run_{name}()?\n");
            let results = any.iter().map(|&a| reference(a)).collect();
            cases.push(Case {
                name,
                ty,
                marker,
                results,
            });
        }
        // A cast from each other type, of every value both types have.
        for (from, from_bits, from_signed) in WIDTHS {
            if from == ty {
                continue;
            }
            let (from_min, from_max) = limits(from_bits, from_signed);
            let shared = (min.max(from_min), max.min(from_max));
            let name = format!("cast_{from}_{ty}");
            source += &format!("def {name}(a: {name}_T0) -> {ty}:\n    {ty}(a.get())\n");
            let values = edge_values(shared.0, shared.1);
            emit(
                &mut source,
                &mut main,
                &name,
                &[(from, shared, from_min, &values)],
            );
            cases.push(Case {
                name,
                ty,
                marker: "Proved(CheckedCast(",
                results: values,
            });
        }
    }
    for case in &cases {
        for result in &case.results {
            expected += &format!("{result}\n");
        }
    }
    source += &format!("def main() -> Result[(), AllocError | IoError]:\n{main}    Ok(())\n");

    let operations = operations(&source);
    for case in &cases {
        let body = &operations[&case.name];
        assert!(body.contains(case.marker), "{}:\n{body}", case.name);
        assert!(
            !body.contains("Checked") || case.marker.starts_with("Proved(CheckedCast("),
            "{}:\n{body}",
            case.name
        );
        let (_, bits, signed) = WIDTHS.into_iter().find(|width| width.0 == case.ty).unwrap();
        let (min, max) = limits(bits, signed);
        // The interval recorded for the operator under test.
        if case.marker.starts_with("Proved(") {
            let proof = body
                .lines()
                .rev()
                .find(|line| line.starts_with(case.marker))
                .unwrap();
            let (lo, hi) = proof
                .split_once("Interval { lo: ")
                .and_then(|(_, interval)| interval.split_once(", hi: "))
                .unwrap();
            let lo: i128 = lo.parse().unwrap();
            let hi: i128 = hi.trim_end_matches(" })").parse().unwrap();
            assert!(min <= lo && hi <= max, "{}: {proof}", case.name);
            for result in &case.results {
                assert!(
                    (lo..=hi).contains(result),
                    "{}: {result} {proof}",
                    case.name
                );
            }
        }
    }
    for flags in [&[][..], &["--release"]] {
        let actual = stdout_with(&source, flags);
        if actual != expected {
            let line = actual
                .lines()
                .zip(expected.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(actual.lines().count().min(expected.lines().count()));
            let mut seen = 0;
            let case = cases
                .iter()
                .find(|case| {
                    seen += case.results.len();
                    line < seen
                })
                .map_or("past the end", |case| case.name.as_str());
            panic!(
                "{flags:?}: line {line} ({case}): got {:?}, expected {:?}",
                actual.lines().nth(line),
                expected.lines().nth(line)
            );
        }
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
        // A loop over a range value, a pattern binding and a field have the
        // full range; a direct `range` loop is in `a_range_loop_counter_*`.
        "r = range(3)\n    for i in r:\n        return i * 2\n    0",
        "pair = (x % 10, 1)\n    (first, _) = pair\n    first * 2",
    ] {
        rejected(
            &format!("def f(x: i64) -> i64:\n    {body}\ndef main() -> ():\n    pass\n"),
            "arithmetic can fail with OverflowError[i64]",
        );
    }
    // `!=` narrows nothing; other comparisons are in `a_comparison_*`.
    rejected(
        "def f(x: i64) -> i64:
    if x != 0 and x != 10:
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
    // `unchecked:` traps if the addition overflows; the result of an addition
    // that can fail still has the full range.
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
    // A `checked:` block with a proved and an unproved operation.
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

// A `checked:` block's error type is the one its operators have with no
// interval proof, so a block compiles the same whether or not its
// arithmetic is proved (issue #952, PR #1215 comments 2903 and 2904).
const CHECKED_BLOCKS: &str = "\
def all_proved(a: u8, b: u8) -> i64:
    r = checked:
        i64(a) + i64(b) * 2
    match r:
        case Ok(v):
            v
        case Err(OverflowError(w)):
            w

def shifts(a: u8, n: u8) -> i64:
    r = checked:
        k = n % 8
        (i64(a) << i64(k)) // (i64(k) + 1)
    match r:
        case Ok(v):
            v
        case Err(ShiftCountError(c)):
            c
        case Err(ZeroDivisionError()):
            -1
        case Err(OverflowError(w)):
            w

def unit(a: u8) -> bool:
    r = checked:
        x = i64(a) + 1
        drop(x)
    match r:
        case Ok(_):
            True
        case Err(_):
            False

def payload(a: u8, d: i64) -> i64:
    r = checked:
        (i64(a) + 1) // d
    match r:
        case Ok(v):
            v
        case Err(OverflowError(w)):
            w
        case Err(_):
            0

def mixed(a: u8, d: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    checked:
        (i64(a) + 1) // d + i64.MAX

def gen[T: IntType](a: T) -> Result[T, OverflowError[T] | ZeroDivisionError]:
    checked:
        a % T(100) + T(100)

def main() -> Result[(), IoError]:
    print(all_proved(3u8, 4u8), shifts(3u8, 10u8), unit(1u8))?
    print(payload(3u8, 2), payload(3u8, 0), payload(255u8, -1))?
    print(mixed(3u8, 2).is_err(), mixed(3u8, 0).is_err(), mixed(3u8, 5).unwrap_or(-1) == i64.MAX)?
    print(gen(i8(50)).is_err(), gen(i16(50)).unwrap_or(i16(0)), gen(-7i64).unwrap_or(0))?
    Ok(())
";

#[test]
fn a_checked_block_has_the_error_type_it_has_without_interval_proofs() {
    runs(
        CHECKED_BLOCKS,
        "11 4 True\n2 0 -256\nTrue True True\nTrue 150 193\n",
    );
    // The proved operations are still emitted without their checks.
    assert_eq!(checks(&clif(CHECKED_BLOCKS, "all_proved")).0, 0);
    // Only the zero divisor is tested: `0..255 << 0..7` and `k + 1` are
    // proved, and the quotient of a nonnegative dividend cannot overflow.
    assert_eq!(checks(&clif(CHECKED_BLOCKS, "shifts")).0, 0);
    assert_eq!(checks(&clif(CHECKED_BLOCKS, "mixed")).0, 1);

    // One generic body is accepted for every instantiation, with the same
    // error type, although `i16` proves the addition and `i8` does not.
    let generic = "def f[T: IntType](a: T) -> T:
    r = checked:
        a % T(100) + T(100)
    match r:
        case Ok(v):
            v
        case Err(OverflowError(w)):
            w
        case Err(ZeroDivisionError()):
            T(0)
def main() -> Result[(), IoError]:
    print(f(i8(50)), f(i16(5)), f(-7i64))?
    Ok(())
";
    runs(generic, "-106 105 193\n");

    // An arm for an error no operation produces at run time is dead code,
    // and so is a `?` that propagates it.
    accepted(
        "def f(a: u8) -> Result[i64, OverflowError[i64]]:
    r = checked:
        i64(a) * 2
    Ok(r?)
def main() -> ():
    drop(f(3u8))
",
    );
}

#[test]
fn a_checked_block_without_fallible_operators_is_still_rejected() {
    let empty = "this `checked:` block contains no arithmetic that can fail";
    for body in [
        // Infallible before intervals: a literal divisor of an unsigned
        // dividend, a widening conversion, a literal shift count, a
        // masked count, a comparison.
        "x // 2u64",
        "u64(u8(3)) | x",
        "x << 3",
        "x >> (x & 63u64)",
        "x == 2u64",
    ] {
        rejected(
            &format!(
                "def f(x: u64) -> ():\n    r = checked:\n        {body}\n    drop(r)\ndef main() -> ():\n    f(1u64)\n"
            ),
            empty,
        );
    }
    // A proved operator is arithmetic the block checks, so each of these
    // is accepted with the error type of the unproved operator.
    for (body, error) in [
        ("i64(a) + 1", "OverflowError[i64]"),
        ("i64(a) - 1", "OverflowError[i64]"),
        ("i64(a) * 2", "OverflowError[i64]"),
        ("-i64(a)", "OverflowError[i64]"),
        (
            "7 // (i64(a) + 1)",
            "OverflowError[i64] | ZeroDivisionError",
        ),
        ("7 % (i64(a) + 1)", "OverflowError[i64] | ZeroDivisionError"),
        ("1 << i64(a % 8u8)", "ShiftCountError[i64]"),
    ] {
        accepted(&format!(
            "def f(a: u8) -> Result[i64, {error}]:\n    checked:\n        {body}\ndef main() -> ():\n    drop(f(3u8))\n"
        ));
    }
}

// A `Proved` interval is derived inside one function body from parameters
// that have the full range of their type, so it holds for every caller the
// body is inlined into; a generic body is checked once per instantiation.
#[test]
fn proofs_hold_for_every_caller_and_are_made_per_instantiation() {
    let source = r#"def widen[T: IntType](a: u8) -> Result[T, OverflowError[T]]:
    Ok(T(a) + T(100))

def twice[T: IntType](a: T) -> Result[T, OverflowError[T]]:
    half = a // T(2)
    Ok(half + half)

def step(n: i64) -> i64:
    n % 10 + 1

def small(n: u8) -> i64:
    i64(n) * 1000

def caller(n: i64) -> Result[i64, OverflowError[i64]]:
    x = step(n)
    y = small(255u8)
    Ok(step(i64.MAX) + step(i64.MIN) + x * y + small(u8(x)))

def main() -> Result[(), IoError]:
    print(widen[i8](100u8).is_err(), widen[i16](100u8).unwrap_or(i16(0)), widen[u8](200u8).is_err(), widen[i64](255u8).unwrap_or(0))?
    print(twice(i8(-128)).unwrap_or(i8(0)), twice(i8(127)).unwrap_or(i8(0)), twice(255u8).unwrap_or(0u8), twice(i64.MIN).unwrap_or(0))?
    print(caller(-1).unwrap_or(-1), caller(i64.MAX).unwrap_or(-1))?
    Ok(())
"#;
    runs(
        source,
        "True 200 True 355\n-128 126 254 -9223372036854775808\n2560011 2048011\n",
    );
    // `step` and `small` are proved for every argument. The checker gives
    // a call's result the full range, so `caller` declares its error; code
    // generation's own bounds then cover the inlined results.
    assert_eq!(checks(&clif(source, "caller")).0, 0);
}

// A proved operation in an `unchecked:` suite is the same plain operation,
// and the suite still contributes no error to an enclosing `checked:` block.
#[test]
fn unchecked_suites_keep_their_contract() {
    runs(
        r#"def all_proved(a: u8, b: u8) -> i64:
    unchecked:
        i64(a) + i64(b) * 2

def partly(a: u8, d: i64) -> i64:
    unchecked:
        (i64(a) + 1) // d + (i64(a) << 3)

def nested(a: u8, d: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    checked:
        x = unchecked:
            i64(a) * 3
        y = unchecked:
            x // d
        y + x

def inner(a: u8, d: i64) -> i64:
    unchecked:
        r = checked:
            i64(a) + 1
        r.unwrap_or(0) // d

def main() -> Result[(), IoError]:
    print(all_proved(3u8, 4u8), partly(3u8, 2), nested(3u8, 2).unwrap_or(-1), inner(9u8, 5))?
    Ok(())
"#,
        "11 26 13 2\n",
    );
    rejected(
        r#"def f(a: u8, d: i64) -> Result[i64, OverflowError[i64]]:
    checked:
        unchecked:
            i64(a) * 3
def main() -> ():
    drop(f(1u8, 2))
"#,
        "this `checked:` block contains no arithmetic that can fail",
    );
}

// A literal operand of an integer domain type is checked by value (issue
// #955); the interval of the forwarded arithmetic removes a check only where
// the plain integer operation cannot fail.
#[test]
fn domain_literal_operands_keep_their_checks() {
    let source = r#"class Scale:
    by: NonZero[i64]
    up: Positive[i64]
    count: NonNegative[i8]

def bump(s: &mut Scale) -> Result[(), OverflowError[i64] | OverflowError[i8]]:
    s.by *= 1
    s.by *= -1
    s.up += 1
    s.up *= 2
    s.count += 0
    s.count += 100
    Ok(())

def div(n: i64, p: Positive[i64], z: NonZero[i64]) -> Result[i64, OverflowError[i64]]:
    Ok(n // p + n % p + n % z + (n % 7) // z)

def lit(n: i64) -> i64:
    n // -(-3) + n % -(-(4))

def main() -> Result[(), IoError]:
    mut s = Scale(-9223372036854775807, 4611686018427387902, 27)
    print(bump(&mut s).is_ok(), s.by, s.up, s.count)?
    print(bump(&mut s).is_err(), s.by, s.up, s.count)?
    mut t = Scale(-3, 1, 56)
    print(bump(&mut t).is_err(), t.by, t.up, t.count)?
    print(div(-7, 2, -1).unwrap_or(0), div(i64.MIN, 1, -1).unwrap_or(0), lit(-7))?
    Ok(())
"#;
    runs(
        source,
        "True 9223372036854775807 9223372036854775806 127\n\
         True -9223372036854775807 9223372036854775807 127\n\
         True 3 4 56\n-3 0 -2\n",
    );
    // An interval is not a domain proof: only a literal converts.
    rejected(
        "def f(a: u8) -> NonZero[i64]:
    i64(a) + 1
def main() -> ():
    pass
",
        "expected NonZero[i64], got i64",
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

/// The function `f`, whose parameters `x: i64`, `y: i64`, `u: u8`,
/// `s: Bounded[i64, -5, 7]` and `k: NonZero[i64]` reach `body`, which
/// returns the value of type `ty` that it marks `BOUNDED` from inside
/// `Bounded[ty, lo, hi].from(...)`; `LOWEST` returns `lo`.
fn region(ty: &str, body: &str, lo: i128, hi: i128) -> String {
    let returns = body.rsplit('\n').next().unwrap().starts_with("    return");
    let fallback = if returns { "" } else { "\n    return LOWEST" };
    let body = format!("{body}{fallback}")
        .replace("BOUNDED", &format!("Bounded[{ty}, {lo}, {hi}].from"))
        .replace(
            "LOWEST",
            &format!("Bounded[{ty}, {lo}, {hi}].from({ty}({lo}))"),
        );
    format!(
        "def f(x: i64, y: i64, u: u8, s: Bounded[i64, -5, 7], k: NonZero[i64]) -> Bounded[{ty}, {lo}, {hi}]:
    {body}
def main() -> ():
    pass
"
    )
}

/// `body` proves exactly `lo..hi` for the value it marks.
fn region_is(ty: &str, body: &str, lo: i128, hi: i128) {
    accepted(&region(ty, body, lo, hi));
    let named = format!("this value is in {lo}..{hi}, which is not inside");
    if lo < hi {
        rejected(&region(ty, body, lo + 1, hi), &named);
        rejected(&region(ty, body, lo, hi - 1), &named);
    }
}

#[test]
fn a_range_loop_counter_lies_between_its_start_and_stop() {
    let max = i128::from(i64::MAX);
    let min = i128::from(i64::MIN);
    for (header, lo, hi) in [
        ("range(10)", 0, 9),
        ("range(3, 10)", 3, 9),
        ("range(s.get(), 10)", -5, 9),
        ("range(-5, s.get())", -5, 6),
        ("range(x)", 0, max - 1),
        ("range(x, 0)", min, -1),
        // A literal or constant step keeps the interval, and a negative one
        // mirrors it; `reversed` gives the same elements.
        ("range(0, 10, 3)", 0, 9),
        ("range(10, 0, -1)", 1, 10),
        ("range(10, s.get(), -2)", -4, 10),
        ("reversed(range(3, 10))", 3, 9),
    ] {
        region_is(
            "i64",
            &format!("for i in {header}:\n        return BOUNDED(i)"),
            lo,
            hi,
        );
    }
    region_is(
        "i64",
        "for i in range(10):\n        for j in range(i):\n            return BOUNDED(j)",
        0,
        8,
    );
    region_is(
        "u8",
        "for i in range(u, 200u8):\n        return BOUNDED(i)",
        0,
        199,
    );
    // A step that is not a literal or constant, a loop over a range value
    // and an empty range give the full range.
    let full = "this value is in -9223372036854775808..9223372036854775807";
    for body in [
        "for i in range(0, 10, k):\n        return BOUNDED(i)",
        "r = range(10)\n    for i in r:\n        return BOUNDED(i)",
        "for i in range(10, 0):\n        return BOUNDED(i)",
    ] {
        rejected(&region("i64", body, 0, 9), full);
    }
    // An inner loop of the same name has its own interval.
    rejected(
        &region(
            "i64",
            "for i in range(10):\n        for i in range(x, y):\n            return BOUNDED(i)",
            0,
            9,
        ),
        "this value is in -9223372036854775808..9223372036854775806",
    );
}

#[test]
fn a_range_loop_counter_has_no_check_where_its_interval_proves_the_operation() {
    let source = "\
def squares(n: Bounded[i64, 0, 3000000]) -> i64:
    mut total = 0
    for i in range(n.get()):
        total = total ^ (i * i + 1)
    total

def doubled(n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(n - 2, n):
        total = total ^ (i * 2)
    Ok(total)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(squares(5), doubled(10)?, doubled(i64.MAX).is_err())?
    Ok(())
";
    runs(source, "29 2 True\n");
    assert_eq!(checks(&clif(source, "squares")).0, 0);
    // `n - 2`, and `i * 2`, which a stop of any i64 does not bound, keep
    // their checks, and the multiplication fails at run time.
    assert_eq!(checks(&clif(source, "doubled")).0, 2);
}

#[test]
fn a_comparison_narrows_an_immutable_local_where_it_holds() {
    let max = i128::from(i64::MAX);
    let min = i128::from(i64::MIN);
    for (body, lo, hi) in [
        ("if x >= 0 and x < 10:\n        return BOUNDED(x)", 0, 9),
        // Either operand order.
        ("if 0 <= x and 10 > x:\n        return BOUNDED(x)", 0, 9),
        ("if x == 7:\n        return BOUNDED(x)", 7, 7),
        ("if x == s.get():\n        return BOUNDED(x)", -5, 7),
        (
            "if s.get() < x and x <= 20:\n        return BOUNDED(x)",
            -4,
            20,
        ),
        ("if x > i64.MAX - 1:\n        return BOUNDED(x)", max, max),
        // A comparison between two locals bounds both.
        (
            "if y >= 0 and y < 50 and x < y and x >= 0:\n        return BOUNDED(x)",
            0,
            48,
        ),
        (
            "if x >= 0 and x < 50 and y > x and y < 100:\n        return BOUNDED(y)",
            1,
            99,
        ),
        // `not`, and the else suite of `or` by its negation.
        ("if not (x < 0 or x > 9):\n        return BOUNDED(x)", 0, 9),
        (
            "if x < 0 or x > 9:\n        pass\n    else:\n        return BOUNDED(x)",
            0,
            9,
        ),
        // An elif holds after the earlier conditions fail.
        (
            "if x < 0:\n        pass\n    elif x < 10:\n        return BOUNDED(x)",
            0,
            9,
        ),
        // A while body.
        ("while x > 0 and x < 5:\n        return BOUNDED(x)", 1, 4),
        // Each suite that exits early narrows the rest of its suite.
        (
            "if x < 0:\n        return LOWEST\n    if x > 9:\n        return LOWEST\n    return BOUNDED(x)",
            0,
            9,
        ),
        (
            "if x < 0 or x > 9:\n        return LOWEST\n    return BOUNDED(x)",
            0,
            9,
        ),
        (
            "if x > 9:\n        return LOWEST\n    elif x < 0:\n        return LOWEST\n    return BOUNDED(x)",
            0,
            9,
        ),
        (
            "for i in range(10):\n        if x < i:\n            continue\n        return BOUNDED(x)",
            0,
            max,
        ),
        // Only the then suite continues.
        (
            "if x >= 0 and x < 3:\n        pass\n    else:\n        return LOWEST\n    return BOUNDED(x)",
            0,
            2,
        ),
        // A loop counter and a parameter: `range` and the comparison meet.
        (
            "for i in range(100):\n        if i > 4:\n            return BOUNDED(i)",
            5,
            99,
        ),
        // A bound beyond the type's range proves nothing more.
        ("if x < i64.MIN + 1:\n        return BOUNDED(x)", min, min),
    ] {
        region_is("i64", body, lo, hi);
    }
    region_is("u8", "if u > 250u8:\n        return BOUNDED(u)", 251, 255);
    region_is("u8", "if u <= 3u8:\n        return BOUNDED(u)", 0, 3);
    // A suite that cannot run keeps the interval.
    region_is("u8", "if u < 0u8:\n        return BOUNDED(u)", 0, 255);
    // The right operand of `and` runs only when the left one holds, and
    // that of `or` only when it fails.
    accepted(&infallible(
        "i64",
        "if a > -100 and a < 100 and a * a < 50:\n        return 1\n    0",
    ));
    accepted(&infallible(
        "i64",
        "if a < -100 or a > 100 or a * a < 50:\n        return 1\n    0",
    ));
}

#[test]
fn a_comparison_narrows_nothing_else() {
    let full = "this value is in -9223372036854775808..9223372036854775807";
    for body in [
        // `!=`, and either side of a true `or` or a false `and`.
        "if x != 7:\n        return BOUNDED(x)",
        "if x == 0 or x == 9:\n        return BOUNDED(x)",
        "if x >= 0 and x <= 9:\n        pass\n    else:\n        return BOUNDED(x)",
        // A suite that continues narrows nothing after the `if`.
        "if x >= 0 and x <= 9:\n        pass\n    return BOUNDED(x)",
        // An early exit nested in a suite that continues.
        "if y > 0:\n        if x > 9 or x < 0:\n            return LOWEST\n    return BOUNDED(x)",
        // After a loop, by the failed `while` condition.
        "while x < 0 or x > 9:\n        break\n    return BOUNDED(x)",
        // A mutable local and an element, which may differ on
        // their next read.
        "mut m = x\n    if m >= 0 and m <= 9:\n        return BOUNDED(m)",
        "pair = (x, y)\n    if pair[0] >= 0 and pair[0] <= 9:\n        return BOUNDED(pair[0])",
        // An if-expression's condition.
        "z = x if x >= 0 and x <= 9 else 0\n    return BOUNDED(x)",
    ] {
        let error = check_diagnostic(&region("i64", body, 0, 9));
        assert!(
            error.contains(full) || error.contains("can fail") || error.contains("not inside"),
            "{body}\n{error}"
        );
    }
    rejected(
        &infallible(
            "i64",
            "if a < 0 or a > 9:\n        return a * 1000000000000000000\n    0",
        ),
        "arithmetic can fail with OverflowError[i64]",
    );
}

#[test]
fn a_narrowed_operation_has_no_check_and_a_checked_block_keeps_its_type() {
    let source = "\
def scale(x: i64) -> i64:
    if x < 0 or x > 1000000:
        return 0
    x * x * 1000

def kept(x: i64) -> Result[i64, OverflowError[i64]]:
    if x != 0:
        return Ok(x * 1000)
    Ok(0)

def block(x: i64) -> Result[i64, OverflowError[i64]]:
    if x >= 0 and x < 10:
        r = checked:
            x * 2
        return Ok(r?)
    Ok(0)

def main() -> Result[(), IoError | OverflowError[i64]]:
    print(scale(-1), scale(0), scale(1000000), scale(1000001))?
    print(kept(7)?, kept(i64.MAX).is_err(), block(9)?, block(10)?)?
    Ok(())
";
    runs(source, "0 0 1000000000000000 0\n7000 True 18 0\n");
    assert_eq!(checks(&clif(source, "scale")).0, 0);
    assert_eq!(checks(&clif(source, "kept")).0, 1);
    // The `checked:` block still has its error type, but its proved
    // multiplication has no check.
    assert_eq!(checks(&clif(source, "block")).0, 0);
}

#[test]
fn an_unnarrowed_operation_still_traps_at_run_time() {
    // `n < 10` holds only in the early-return suite; after it, `n` has its
    // full range, so `unchecked:` keeps its trap. Inside the loop, the
    // narrowed `i * 2` runs without one.
    let source = "\
def f(n: i64) -> i64:
    mut total = 0
    for i in range(4):
        if i < 3:
            total = i * 2
    if n < 10:
        return total
    unchecked:
        n * 2

def main() -> Result[(), IoError]:
    print(f(5))?
    print(f(i64.MAX))?
    Ok(())
";
    support::assert_file_traps(source, "overflow");
}

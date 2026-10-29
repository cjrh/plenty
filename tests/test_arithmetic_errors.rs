//! `checked:` blocks: integer arithmetic written inside one exits the block
//! with `Err(OverflowError[T])` or `Err(ZeroDivisionError)` instead of
//! trapping (issue #557). Float `div_checked` rejects both zeros. Arithmetic
//! outside a block still traps. None of it allocates.
mod support;
use support::{check_diagnostic, run_stdout};

/// (type, bits, signed)
const TYPES: [(&str, u32, bool); 8] = [
    ("u8", 8, false),
    ("u16", 16, false),
    ("u32", 32, false),
    ("u64", 64, false),
    ("i8", 8, true),
    ("i16", 16, true),
    ("i32", 32, true),
    ("i64", 64, true),
];

fn bounds(bits: u32, signed: bool) -> (i128, i128) {
    if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// The exact result reduced into the type, as `OverflowError.wrapped` holds it.
fn wrap(value: i128, bits: u32, signed: bool) -> i128 {
    let modulus = 1i128 << bits;
    let reduced = value.rem_euclid(modulus);
    if signed && reduced >= modulus / 2 {
        reduced - modulus
    } else {
        reduced
    }
}

/// A value of `ty` as Plenty source. A minimum is not a literal: its
/// magnitude is out of range.
fn literal(ty: &str, value: i128, min: i128) -> String {
    if value == min && min < 0 {
        format!("{ty}.MIN")
    } else {
        format!("{value}{ty}")
    }
}

/// Plenty prints `Ok` values plainly and errors as `overflow <wrapped>`.
fn outcome(value: i128, bits: u32, signed: bool) -> String {
    let (min, max) = bounds(bits, signed);
    if (min..=max).contains(&value) {
        value.to_string()
    } else {
        format!("overflow {}", wrap(value, bits, signed))
    }
}

/// `match` on `Result[i64, ZeroDivisionError | OverflowError[i64]]`.
const SHOW: &str =
    "def show(result: Result[i64, ZeroDivisionError | OverflowError[i64]]) -> Result[(), IoError]:
    match result:
        case Ok(value):
            print(value)?
        case Err(error):
            match error:
                case zero: ZeroDivisionError:
                    print(\"zero\")?
                case overflow: OverflowError[i64]:
                    print(\"overflow\", overflow.wrapped)?
    Ok(())
";

#[test]
fn overflow_carries_the_wrapped_value_for_every_width() {
    let mut source = String::new();
    let mut expected = String::new();
    for (ty, bits, signed) in TYPES {
        let (min, max) = bounds(bits, signed);
        source.push_str(&format!(
            "def show_{ty}(result: Result[{ty}, OverflowError[{ty}]]) -> Result[(), IoError]:\n    match result:\n        case Ok(value):\n            print(value)?\n        case Err(error):\n            print(\"overflow\", error.wrapped)?\n    Ok(())\n\
             def add_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    checked:\n        a + b\n\
             def sub_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    checked:\n        a - b\n\
             def mul_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    checked:\n        a * b\n\
             def pow_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    checked:\n        a.pow(b)\n"
        ));
        if signed {
            source.push_str(&format!(
                "def neg_{ty}(a: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    checked:\n        -a\n"
            ));
        }
        let cases: Vec<(&str, i128, i128, i128)> = vec![
            ("add", max, 1, max + 1),
            ("add", max - 1, 1, max),
            ("add", 3, 4, 7),
            ("sub", min, 1, min - 1),
            ("sub", min + 1, 1, min),
            ("mul", max, 2, max * 2),
            ("mul", max / 2, 2, max / 2 * 2),
        ];
        for (op, a, b, exact) in cases {
            source.push_str(&format!(
                "    show_{ty}({op}_{ty}({}, {}))?\n",
                literal(ty, a, min),
                literal(ty, b, min)
            ));
            expected.push_str(&outcome(exact, bits, signed));
            expected.push('\n');
        }
        for exponent in [bits - 1, bits] {
            source.push_str(&format!(
                "    show_{ty}(pow_{ty}(2{ty}, {exponent}{ty}))?\n"
            ));
            expected.push_str(&outcome(1i128 << exponent, bits, signed));
            expected.push('\n');
        }
        if signed {
            for value in [min, min + 1] {
                source.push_str(&format!(
                    "    show_{ty}(neg_{ty}({}))?\n",
                    literal(ty, value, min)
                ));
                expected.push_str(&outcome(-value, bits, signed));
                expected.push('\n');
            }
        }
    }
    let (declarations, calls): (Vec<&str>, Vec<&str>) = source
        .lines()
        .partition(|line| !line.starts_with("    show_"));
    let program = format!(
        "{}\ndef main() -> Result[(), IoError]:\n{}\n    Ok(())\n",
        declarations.join("\n"),
        calls.join("\n")
    );
    assert_eq!(run_stdout(&program), expected, "{program}");
}

#[test]
fn division_floors_and_reports_zero_divisors_and_the_one_overflowing_quotient() {
    let mut source = format!(
        "{SHOW}def rem(result: Result[i64, ZeroDivisionError]) -> Result[(), IoError]:\n    match result:\n        case Ok(value):\n            print(value)?\n        case Err(_):\n            print(\"zero\")?\n    Ok(())\n\
         def quotient(a: i64, b: i64) -> Result[i64, ZeroDivisionError | OverflowError[i64]]:\n    checked:\n        a // b\n\
         def remainder(a: i64, b: i64) -> Result[i64, ZeroDivisionError]:\n    checked:\n        a % b\n\
         def main() -> Result[(), IoError]:\n",
    );
    let mut expected = String::new();
    let min = i64::MIN as i128;
    for (a, b) in [
        (7, 2),
        (-7, 2),
        (7, -2),
        (-7, -2),
        (5, 0),
        (min, -1),
        (min, 1),
    ] {
        let a_text = if a == min {
            "i64.MIN".to_string()
        } else {
            a.to_string()
        };
        source.push_str(&format!(
            "    show(quotient({a_text}, {b}))?\n    rem(remainder({a_text}, {b}))?\n"
        ));
        if b == 0 {
            expected.push_str("zero\nzero\n");
            continue;
        }
        // Floor division: the quotient rounds toward negative infinity.
        let q = a / b - i128::from(a % b != 0 && (a < 0) != (b < 0));
        let r = a - q * b;
        expected.push_str(&if (i64::MIN as i128..=i64::MAX as i128).contains(&q) {
            format!("{q}\n{r}\n")
        } else {
            format!("overflow {}\n{r}\n", wrap(q, 64, true))
        });
    }
    source.push_str("    Ok(())\n");
    assert_eq!(run_stdout(&source), expected, "{source}");
}

#[test]
fn unsigned_division_can_only_fail_on_a_zero_divisor() {
    let output = run_stdout(
        "def main() -> Result[(), IoError]:
    a: u8 = 255
    zero: u8 = 0
    quotient: Result[u8, ZeroDivisionError] = checked:
        a // 2u8
    remainder = checked:
        a % zero
    print(quotient.is_ok(), remainder.is_err())?
    Ok(())
",
    );
    assert_eq!(output, "True True\n");
}

#[test]
fn a_constant_divisor_still_types_the_failure_it_cannot_produce() {
    // The block's type follows its operators, not its operand values; the
    // code generator omits the branches a constant divisor rules out.
    let output = run_stdout(&format!(
        "{SHOW}def main() -> Result[(), IoError]:
    n = i64.MIN
    quarter = checked:
        n // 4
    show(quarter)?
    Ok(())
"
    ));
    assert_eq!(output, "-2305843009213693952\n");
}

#[test]
fn float_div_checked_rejects_only_zero_divisors() {
    let output = run_stdout(
        r#"def show(result: Result[f64, ZeroDivisionError]) -> Result[(), IoError]:
    match result:
        case Ok(value):
            print(value)?
        case Err(_):
            print("zero")?
    Ok(())
def main() -> Result[(), IoError]:
    zero = 0.0
    inf = 1.0 / zero
    nan = zero / zero
    show(1.0.div_checked(zero))?
    show(1.0.div_checked(-zero))?
    show(1.0.div_checked(4.0))?
    show(1.0.div_checked(inf))?
    show(inf.div_checked(2.0))?
    show(1.0.div_checked(nan))?
    print(f32(1.0).div_checked(f32(-0.0)).is_err(), f32(3.0).div_checked(f32(2.0)).unwrap())?
    print(1.0 / zero, -1.0 / zero)?
    Ok(())
"#,
    );
    assert_eq!(
        output,
        "zero\nzero\n0.25\n0.0\ninf\nNaN\nTrue 1.5\ninf -inf\n"
    );
}

#[test]
fn arithmetic_outside_a_block_still_traps() {
    for (expression, message) in [
        ("a + 1", "main.plenty:4:9: integer overflow"),
        ("a // zero", "main.plenty:4:9: division by zero"),
        ("a % zero", "main.plenty:4:9: division by zero"),
    ] {
        let output = support::run_file(&format!(
            "def main() -> ():\n    a = i64.MAX\n    zero = 0\n    b = {expression}\n    drop(b)\n"
        ));
        assert_eq!(output.status.code(), Some(1), "{expression}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(message), "{expression}: {stderr}");
    }
}

#[test]
fn a_block_combines_its_failures_and_question_mark_still_exits_the_function() {
    let output = run_stdout(&format!(
        "{SHOW}def mean(text: &str, count: i64) -> Result[Result[i64, ZeroDivisionError | OverflowError[i64]], ParseError | RangeError]:
    result = checked:
        total = i64.parse(text)?
        total * 2 // count
    Ok(result)
def main() -> Result[(), IoError | ParseError | RangeError]:
    show(mean(\"9\", 2)?)?
    show(mean(\"9\", 0)?)?
    show(mean(\"9223372036854775807\", 1)?)?
    match mean(\"nine\", 1):
        case Ok(_):
            print(\"parsed\")?
        case Err(_):
            print(\"not a number\")?
    Ok(())
"
    ));
    assert_eq!(output, "9\nzero\noverflow -2\nnot a number\n");
}

#[test]
fn a_declared_wider_error_is_kept() {
    let output = run_stdout(
        "def triple(text: &str) -> Result[i64, OverflowError[i64] | ParseError | RangeError]:
    checked:
        i64.parse(text)? * 3
def main() -> Result[(), IoError]:
    show(triple(\"4\"))?
    show(triple(\"9223372036854775807\"))?
    show(triple(\"x\"))?
    Ok(())
def show(result: Result[i64, OverflowError[i64] | ParseError | RangeError]) -> Result[(), IoError]:
    match result:
        case Ok(value):
            print(value)?
        case Err(error):
            match error:
                case overflow: OverflowError[i64]:
                    print(\"overflow\", overflow.wrapped)?
                case _:
                    print(\"other\")?
    Ok(())
",
    );
    assert_eq!(output, "12\noverflow 9223372036854775805\nother\n");
}

#[test]
fn return_break_and_continue_leave_the_block_normally() {
    let output = run_stdout(
        "def first_large(xs: &list[i64]) -> Result[i64, OverflowError[i64]]:
    for i in range(len(xs)):
        if xs[i] > 10:
            return Ok(xs[i])
    checked:
        if len(xs) == 0:
            return Ok(0)
        xs[0] + 1
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 20]?
    empty: list[i64] = []?
    print(first_large(&xs).unwrap(), first_large(&empty).unwrap())?
    mut i: i64 = 0
    mut seen: i64 = 0
    while True:
        i += 1
        step = checked:
            if i == 2:
                continue
            if i == 5:
                break
            seen * 10 + i
        seen = step.unwrap()
    print(i, seen)?
    Ok(())
",
    );
    assert_eq!(output, "20 0\n5 134\n");
}

#[test]
fn the_innermost_block_catches_and_calls_keep_trapping() {
    let output = run_stdout(
        "def square(a: i64) -> i64:
    a * a
def nested(a: i64) -> Result[(), IoError]:
    outer = checked:
        inner = checked:
            a * a
        match inner:
            case Ok(value):
                print(\"inner\", value)?
            case Err(error):
                print(\"inner overflow\", error.wrapped)?
        a + 1
    match outer:
        case Ok(value):
            print(\"outer\", value)?
        case Err(error):
            print(\"outer overflow\", error.wrapped)?
    Ok(())
def main() -> Result[(), IoError]:
    nested(3)?
    nested(i64.MAX)?
    Ok(())
",
    );
    assert_eq!(
        output,
        "inner 9\nouter 4\ninner overflow 1\nouter overflow -9223372036854775808\n"
    );
    // A function or closure called from a block is not lexically inside it.
    for callee in [
        "square(i64.MAX)",
        "apply(&(def(v: i64) -> i64: v * 2), i64.MAX)",
    ] {
        let output = support::run_file(&format!(
            "def square(a: i64) -> i64:\n    a * a\n\
             def apply[F: Callable[[i64], i64]](f: &F, value: i64) -> i64:\n    f(value)\n\
             def main() -> ():\n    r = checked:\n        {callee} + 1\n    drop(r)\n"
        ));
        assert_eq!(output.status.code(), Some(1), "{callee}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("integer overflow"), "{callee}: {stderr}");
    }
}

#[test]
fn compound_assignment_and_forwarded_operators_are_checked() {
    let output = run_stdout(
        "@newtype
class Meters:
    value: i32
def main() -> Result[(), AllocError | IoError]:
    counted = checked:
        mut x: u8 = 200
        x += 50
        x *= 2
        x
    match counted:
        case Ok(value):
            print(value)?
        case Err(error):
            print(\"u8 overflow\", error.wrapped)?
    mut xs = [i64.MAX]?
    bumped = checked:
        xs[0] += 1
        xs[0]
    print(bumped.is_err(), xs[0])?
    far = Meters(2000000000)
    sum = checked:
        far + far
    match sum:
        case Ok(total):
            print(total.value)?
        case Err(error):
            print(\"i32 overflow\", error.wrapped)?
    Ok(())
",
    );
    assert_eq!(
        output,
        "u8 overflow 244\nTrue 9223372036854775807\ni32 overflow -294967296\n"
    );
}

#[test]
fn a_negative_exponent_still_traps() {
    let output = support::run_file(
        "def main() -> ():\n    base = 2\n    exponent = -1\n    r = checked:\n        base.pow(exponent)\n    drop(r)\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("negative integer exponent"), "{stderr}");
}

#[test]
fn blocks_that_cannot_fail_or_never_finish_are_rejected() {
    for (body, message) in [
        (
            "        y = i << 3\n        y",
            "this `checked:` block contains no arithmetic that can fail",
        ),
        (
            "        x = i + 2\n        break",
            "every path through this `checked:` block exits",
        ),
    ] {
        let error = check_diagnostic(&format!(
            "for i in range(3):\n    r = checked:\n    {}\n    drop(r)",
            body.replace('\n', "\n    ")
        ));

        assert!(error.contains(message), "{body}: {error}");
    }
    let error = check_diagnostic("x = 5\nr = checked:\n    y = x + 1\n    &x\ndrop(r)");

    assert!(
        error.contains("a `checked:` block cannot produce a reference"),
        "{error}"
    );
    let error = check_diagnostic("a = 4\nb = a.div_checked(2)");

    assert!(error.contains("div_checked requires a float"), "{error}");
    // `checked` is still an ordinary name.
    assert_eq!(
        run_stdout(
            "def main() -> Result[(), IoError]:\n    checked: i64 = 3\n    print(checked + 1)?\n    Ok(())\n"
        ),
        "4\n"
    );
}

// The runtime checks report any value a failure path leaks or releases twice.
#[test]
fn an_early_exit_releases_block_locals_and_pending_operands() {
    let output = run_stdout(
        "def build(n: i64) -> Result[Result[i64, OverflowError[i64]], AllocError]:
    result = checked:
        names = [\"a long string that is heap allocated\", \"another long string\"]?
        doubled = [x * 2 for x in [n, n]?]?
        len(names) + doubled[0] + n * n
    Ok(result)
def main() -> Result[(), AllocError | IoError]:
    for n in [3, i64.MAX, 5000000000]?:
        match build(n)?:
            case Ok(value):
                print(value)?
            case Err(error):
                print(\"overflow\", error.wrapped)?
    Ok(())
",
    );
    assert_eq!(output, "17\noverflow -2\noverflow 6553255926290448384\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn errors_are_built_and_matched_without_allocation() {
    let output = support::run(
        r#"def step(a: i64, b: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    checked:
        a * 2 // b
def main() -> ():
    mut seen = 0
    print("__test_begin_no_allocations__").unwrap()
    for b in range(-2, 3):
        match step(i64.MAX, b):
            case Ok(value):
                seen += value
            case Err(error):
                match error:
                    case zero: ZeroDivisionError:
                        seen += 1
                    case overflow: OverflowError[i64]:
                        seen += overflow.wrapped
    for b in range(-2, 3):
        match step(i64.MAX, b):
            case Ok(value):
                seen -= value
            case Err(ZeroDivisionError()):
                seen -= 1
            case Err(OverflowError(wrapped)):
                seen -= wrapped
    match 1.0.div_checked(0.0):
        case Ok(_):
            pass
        case Err(_):
            seen += 1
    print("__test_end_no_allocations__").unwrap()
    print(seen).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().last(), Some("1"), "{stdout}");
}

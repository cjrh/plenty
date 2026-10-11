//! Numeric built-ins: the methods on a number, the constants `T.MIN`,
//! `T.MAX`, `T.PI`, and `T.E`, and the `FloatType` constraint (issue #34).
mod support;
use rstest::rstest;

const INTEGERS: [(&str, i128, i128); 8] = [
    ("i8", i8::MIN as i128, i8::MAX as i128),
    ("i16", i16::MIN as i128, i16::MAX as i128),
    ("i32", i32::MIN as i128, i32::MAX as i128),
    ("i64", i64::MIN as i128, i64::MAX as i128),
    ("u8", 0, u8::MAX as i128),
    ("u16", 0, u16::MAX as i128),
    ("u32", 0, u32::MAX as i128),
    ("u64", 0, u64::MAX as i128),
];

/// Print each expression on its own line and compare the lines.
fn assert_lines(declarations: &str, setup: &str, cases: &[(&str, &str)]) {
    let mut source = format!("{declarations}def main() -> Result[(), Failure]:\n{setup}");
    let mut expected = String::new();
    for (expression, value) in cases {
        source.push_str(&format!("    print({expression})?\n"));
        expected.push_str(&format!("{value}\n"));
    }
    source.push_str("    Ok(())\n");
    let output = support::run(&source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

fn rejection(body: &str) -> String {
    support::check_source(&format!("def main() -> ():\n{body}"))
        .unwrap_err()
        .to_string()
}

/// What a program that must trap writes to standard error.
fn trap(body: &str) -> String {
    let output = support::run(&format!(
        "def main() -> Result[(), Failure]:\n{body}    Ok(())\n"
    ));
    assert_eq!(output.status.code(), Some(1), "{body}");
    assert!(output.stdout.is_empty(), "{body}");
    let error = String::from_utf8(output.stderr).unwrap();
    error
        .strip_prefix("error: ")
        .unwrap_or_else(|| panic!("{error}"))
        .trim_end()
        .to_owned()
}

#[test]
fn every_integer_type_names_its_limits() {
    let cases: Vec<(String, String)> = INTEGERS
        .iter()
        .flat_map(|(ty, min, max)| {
            [
                (format!("{ty}.MIN"), min.to_string()),
                (format!("{ty}.MAX"), max.to_string()),
            ]
        })
        .collect();
    let cases: Vec<(&str, &str)> = cases
        .iter()
        .map(|(expression, value)| (expression.as_str(), value.as_str()))
        .collect();
    assert_lines("", "", &cases);
}

#[test]
fn constants_are_values_of_their_type() {
    assert_lines(
        "type Byte = u8\ndef headroom[T: IntType](x: T) -> T:\n    T.MAX - x\n",
        "    small: u8 = 7\n",
        &[
            ("u8.MAX - small", "248"),
            ("i8.MAX + 0", "127"),
            ("Byte.MAX", "255"),
            ("headroom(100i8)", "27"),
            ("headroom(1u16)", "65534"),
            ("f64.PI", "3.141592653589793"),
            ("f64.E", "2.718281828459045"),
            ("f32.PI", "3.1415927"),
            ("f32.E", "2.7182817"),
            ("2.0 * f64.PI", "6.283185307179586"),
            ("2.0f32 * f32.PI", "6.2831855"),
        ],
    );
}

#[rstest]
#[case(
    "    x = i64.max\n",
    "2:9: unknown constant `i64.max`; integer types provide `MIN` and `MAX`, float types provide `PI` and `E`"
)]
#[case(
    "    x = f64.MAX\n",
    "2:9: unknown constant `f64.MAX`; integer types provide `MIN` and `MAX`, float types provide `PI` and `E`"
)]
#[case(
    "    x = u8.PI\n",
    "2:9: unknown constant `u8.PI`; integer types provide `MIN` and `MAX`, float types provide `PI` and `E`"
)]
#[case("    x: u8 = i64.MAX\n", "2:5: expected u8, got i64")]
fn unknown_constants_are_rejected(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(rejection(body), expected);
}

#[test]
fn a_literal_exponent_takes_the_receiver_type() {
    assert_lines(
        "",
        "    n = 3u8\n    square: u8 = n.pow(2)\n    below = u8.MAX - 5\n",
        &[("square", "9"), ("below", "250"), ("1u8 + n.pow(2)", "10")],
    );
}

#[test]
fn float_methods_follow_the_platform_math_library() {
    assert_lines(
        "def hypotenuse[T: FloatType](a: T, b: T) -> T:\n    (a * a + b * b).sqrt()\n",
        "    x = 1.0\n    zero = 0.0\n    r = &x\n",
        &[
            ("(9.0).sqrt()", "3.0"),
            ("(2.0f32).sqrt()", "1.4142135"),
            ("(-1.0).sqrt()", "NaN"),
            ("zero.exp()", "1.0"),
            ("(1.0f32).exp()", "2.7182817"),
            ("f64.E.log()", "1.0"),
            ("zero.log()", "-inf"),
            ("(-1.0).log()", "NaN"),
            ("zero.sin()", "0.0"),
            ("zero.cos()", "1.0"),
            ("x.sin() * x.sin() + x.cos() * x.cos()", "1.0"),
            ("(2.5).floor()", "2.0"),
            ("(-2.5).floor()", "-3.0"),
            ("(2.5).ceil()", "3.0"),
            ("(-2.5).ceil()", "-2.0"),
            ("(0.5f32).floor()", "0.0"),
            ("(0.5f32).ceil()", "1.0"),
            ("(1.0 / zero).floor()", "inf"),
            ("(2.0).pow(10.0)", "1024.0"),
            ("(2.0).pow(-1.0)", "0.5"),
            ("(2.0).pow(3)", "8.0"),
            ("(2.0f32).pow(0.5)", "1.4142135"),
            ("zero.pow(0.0)", "1.0"),
            ("(-8.0).pow(1.0 / 3.0)", "NaN"),
            ("(7.5).fmod(2.0)", "1.5"),
            ("(-7.5).fmod(2.0)", "-1.5"),
            ("(7.5).fmod(-2.0)", "1.5"),
            ("(9.0f32).fmod(4.0f32)", "1.0"),
            ("(5.5).fmod(zero)", "NaN"),
            ("r.sqrt()", "1.0"),
            ("hypotenuse(3.0, 4.0)", "5.0"),
            ("hypotenuse(6.0f32, 8.0f32)", "10.0"),
        ],
    );
}

#[test]
fn integer_pow_is_exact_within_the_type() {
    assert_lines(
        "def cube[T: IntType](x: T) -> T:\n    x.pow(3)\n",
        "    n = 3\n    two = 2u8\n",
        &[
            ("n.pow(4)", "81"),
            ("n.pow(0)", "1"),
            ("(0).pow(0)", "1"),
            ("(0).pow(5)", "0"),
            ("(-3).pow(3)", "-27"),
            ("(2).pow(62)", "4611686018427387904"),
            ("(3).pow(39)", "4052555153018976267"),
            ("(2u64).pow(63)", "9223372036854775808"),
            ("(-2i8).pow(7)", "-128"),
            ("(-1i8).pow(127)", "-1"),
            ("(1u8).pow(255)", "1"),
            ("(10u16).pow(4)", "10000"),
            ("two.pow(two)", "4"),
            ("2u8.pow(7)", "128"),
            ("1 + n.pow(2) * 2", "19"),
            ("cube(4u8)", "64"),
            ("cube(-4)", "-64"),
        ],
    );
}

#[rstest]
#[case(
    "    x = (4).sqrt()\n",
    "2:9: sqrt requires a float, got i64; convert with `f64(...)`"
)]
#[case(
    "    n = 4u8\n    x = n.floor()\n",
    "3:9: floor requires a float, got u8; convert with `f64(...)`"
)]
#[case(
    "    n = 4\n    x = n.fmod(3)\n",
    "3:9: fmod requires a float, got i64; convert with `f64(...)`"
)]
#[case("    x = 2.0\n    y = x.sqrt(1.0)\n", "3:9: sqrt takes no arguments")]
#[case(
    "    x = 2.0\n    y = x.pow()\n",
    "3:9: pow takes one argument of the receiver's type"
)]
#[case(
    "    x = 2.0\n    y = x.fmod(1.0, 2.0)\n",
    "3:9: fmod takes one argument of the receiver's type"
)]
#[case(
    "    n = 2\n    y = n.pow(3u8)\n",
    "3:15: cannot combine i64 with u8; convert with `i64(3u8)`"
)]
#[case(
    "    x = 2.0\n    n = 2\n    y = x.pow(n)\n",
    "4:15: cannot combine f64 with i64; convert with `f64(n)`"
)]
#[case("    x = 2.0\n    y = x.tan()\n", "3:9: unsupported method `tan`")]
fn numeric_methods_reject_other_operands(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(rejection(body), expected);
}

#[rstest]
#[case::pow("    n = 2\n    print(1 + n.pow(63))?\n", "3:15: integer overflow")]
#[case::unsigned_pow("    n = 16u8\n    print(n.pow(2))?\n", "3:11: integer overflow")]
#[case::pow_below_the_minimum("    n = -2i8\n    print(n.pow(8))?\n", "3:11: integer overflow")]
#[case::negative_exponent(
    "    n = 2\n    print(n.pow(0 - 1))?\n",
    "3:11: negative integer exponent"
)]
fn integer_results_outside_the_type_trap(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(trap(body), expected);
}

#[test]
fn class_methods_may_reuse_the_method_names() {
    assert_lines(
        "class Account:\n    total: i64\n    def pow(self, n: i64) -> i64:\n        self.total * n\n    def sqrt(self) -> i64:\n        0\n",
        "    account = Account(4)\n",
        &[("account.pow(3)", "12"), ("account.sqrt()", "0")],
    );
}

/// `abs`, `min`, `max`, and `sum` are not built in, so a program may define
/// them.
#[test]
fn programs_may_define_abs_min_max_and_sum() {
    assert_lines(
        "def abs(x: i64) -> i64:\n    if x < 0:\n        return 0 - x\n    x\ndef min(a: i64, b: i64) -> i64:\n    if a < b:\n        return a\n    b\ndef max(a: i64, b: i64) -> i64:\n    if a > b:\n        return a\n    b\ndef sum(xs: &list[i64]) -> i64:\n    mut total = 0\n    for x in xs:\n        total = total + *x\n    total\n",
        "    xs = [3, -4, 5]?\n",
        &[
            ("abs(-7)", "7"),
            ("min(3, -4)", "-4"),
            ("max(3, -4)", "3"),
            ("sum(&xs)", "4"),
            ("max(abs(min(-9, 2)), sum(&xs))", "9"),
        ],
    );
}

#[rstest]
#[case("abs(1)")]
#[case("min(1, 2)")]
#[case("max(1, 2)")]
#[case("sum(xs)")]
fn abs_min_max_and_sum_are_not_built_in(#[case] call: &str) {
    let error = rejection(&format!("    xs = [1, 2].unwrap()\n    x = {call}\n"));
    let name = &call[..3];
    assert_eq!(error, format!("3:9: unknown function `{name}`"));
}

#[test]
fn float_type_admits_exactly_the_float_types() {
    assert_lines(
        "class Reading[T: FloatType]:\n    value: T\nenum Sample[T: FloatType]:\n    Missing\n    Present(T)\ndef half[T: FloatType](x: T) -> T:\n    x / (x / x + x / x)\n",
        "    reading = Reading(1.5f32)\n",
        &[
            ("half(3.0)", "1.5"),
            ("half(3.0f32)", "1.5"),
            ("reading.value", "1.5"),
        ],
    );
    for (declarations, body, expected) in [
        (
            "def half[T: FloatType](x: T) -> T:\n    x\n",
            "    x = half(3)\n",
            "i64 does not satisfy FloatType",
        ),
        (
            "class Reading[T: FloatType]:\n    value: T\n",
            "    x = Reading(3u8)\n",
            "u8 does not satisfy FloatType",
        ),
        (
            "def whole[T: IntType](x: T) -> T:\n    x\n",
            "    x = whole(3.5)\n",
            "f64 does not satisfy IntType",
        ),
        (
            "def any[T: Number](x: T) -> T:\n    x\n",
            "    x = any(3)\n",
            "unknown type `Number`",
        ),
    ] {
        let error = support::check_source(&format!("{declarations}def main() -> ():\n{body}"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
    }
}

//! Numeric built-ins: `abs`, `min`, `max`, `sum`, the methods on a number,
//! the constants `T.MIN`, `T.MAX`, `T.PI`, and `T.E`, and the `FloatType`
//! constraint (issue #34).
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
fn abs_min_and_max_keep_the_operand_type() {
    assert_lines(
        "def largest[T: IntType](a: T, b: T) -> T:\n    max(a, b)\n",
        "    low = -3\n    half = -2.5\n    r = &low\n",
        &[
            ("abs(low)", "3"),
            ("abs(3)", "3"),
            ("abs(half)", "2.5"),
            ("abs(-0.0)", "0.0"),
            ("abs(-1.5f32)", "1.5"),
            ("abs(7u8)", "7"),
            ("abs(i64.MIN + 1)", "9223372036854775807"),
            ("abs(-127i8)", "127"),
            ("abs(*r)", "3"),
            ("min(low, 2)", "-3"),
            ("max(low, 2)", "2"),
            ("min(200u8, 100u8)", "100"),
            ("max(200u8, 100u8)", "200"),
            ("max(-1i8, 1i8)", "1"),
            ("min(u64.MAX, 1u64)", "1"),
            ("min(1.5, 2.5)", "1.5"),
            ("max(1.0f32, 2.0f32)", "2.0"),
            ("min(max(low, 0), 10)", "0"),
            ("min(low, 4) + 1", "-2"),
            ("largest(3u8, 9u8)", "9"),
            ("largest(-3, -9)", "-3"),
        ],
    );
}

#[test]
fn literals_take_their_type_from_the_other_operand_or_the_annotation() {
    assert_lines(
        "",
        "    a: u8 = abs(5)\n    b: f32 = min(1.5, 2.5)\n    c: u8 = max(3, 9)\n    d = max(1, 2) + 3u16\n    e = 1u8 + abs(3) + min(4, 5)\n    n = 3u8\n    f: u8 = n.pow(2)\n",
        &[
            ("a", "5"),
            ("b", "1.5"),
            ("c", "9"),
            ("d", "5"),
            ("e", "8"),
            ("f", "9"),
        ],
    );
}

#[test]
fn float_min_and_max_propagate_nan_and_order_the_zeros() {
    assert_lines(
        "",
        "    zero = 0.0\n    nan = zero / zero\n",
        &[
            ("min(nan, 1.0)", "NaN"),
            ("min(1.0, nan)", "NaN"),
            ("max(nan, 1.0)", "NaN"),
            ("max(1.0, nan)", "NaN"),
            ("min(0.0, -0.0)", "-0.0"),
            ("min(-0.0, 0.0)", "-0.0"),
            ("max(0.0, -0.0)", "0.0"),
            ("max(-0.0, 0.0)", "0.0"),
            ("abs(nan)", "NaN"),
            ("abs(-1.0 / zero)", "inf"),
        ],
    );
}

#[rstest]
#[case(
    "    x = min(1, 2.0)\n",
    "2:13: cannot combine i64 with f64; convert with `f64(1)`"
)]
#[case(
    "    a = 1u8\n    x = max(a, 2i16)\n",
    "3:13: cannot combine u8 with i16; convert with `i16(a)`"
)]
#[case(
    "    x = min(1, 2, 3)\n",
    "2:9: min takes two numbers of one type; nest calls for more, and use a loop over a collection"
)]
#[case(
    "    xs = [1, 2].unwrap()\n    x = max(xs)\n",
    "3:9: max takes two numbers of one type; nest calls for more, and use a loop over a collection"
)]
#[case("    x = max(\"a\", \"b\")\n", "2:13: max requires numbers, got str")]
#[case("    x = abs()\n", "2:9: abs takes one number")]
#[case("    x = abs(1, 2)\n", "2:9: abs takes one number")]
#[case("    x = abs(\"x\")\n", "2:13: abs requires a number, got str")]
#[case("    x = abs(True)\n", "2:13: abs requires a number, got bool")]
fn abs_min_and_max_reject_other_operands(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(rejection(body), expected);
}

#[test]
fn sum_adds_a_list_from_zero_and_leaves_it_usable() {
    assert_lines(
        "class Scores:\n    values: list[u8]\ndef total(xs: &list[i64]) -> i64:\n    sum(xs)\n",
        "    xs = [1, 2, 3]?\n    halves = [1.5, 2.5]?\n    small: list[f32] = [0.5, 0.25]?\n    no_ints: list[u8] = list()?\n    no_floats: list[f64] = list()?\n    scores = Scores([200, 55]?)\n    rows = [[1.5, 2.0]?, [4.0]?]?\n    r = &xs\n    mixed = [100i8, -100i8, 100i8]?\n",
        &[
            ("sum(xs)", "6"),
            ("sum(xs)", "6"),
            ("len(xs)", "3"),
            ("total(&xs)", "6"),
            ("sum(r)", "6"),
            ("sum(halves)", "4.0"),
            ("sum(small)", "0.75"),
            ("sum(no_ints)", "0"),
            ("sum(no_floats)", "0.0"),
            ("sum(scores.values)", "255"),
            ("sum(rows[0])", "3.5"),
            ("sum([1u8, 2u8]?)", "3"),
            ("sum([i * i for i in range(4)]?)", "14"),
            ("sum(mixed)", "100"),
            ("sum(xs) + 1", "7"),
        ],
    );
}

#[rstest]
#[case("    x = sum()\n", "2:9: sum takes one list of numbers")]
#[case(
    "    xs = [1, 2].unwrap()\n    x = sum(xs, 0)\n",
    "3:9: sum takes one list of numbers"
)]
#[case(
    "    x = sum(range(4))\n",
    "2:13: sum requires a list of numbers, got range; add other values in a loop"
)]
#[case(
    "    xs = [\"a\", \"b\"].unwrap()\n    x = sum(xs)\n",
    "3:13: sum requires a list of numbers, got list[str]; add other values in a loop"
)]
#[case(
    "    xs = {1, 2}.unwrap()\n    x = sum(xs)\n",
    "3:13: sum requires a list of numbers, got set[i64]; add other values in a loop"
)]
#[case(
    "    x = sum(3)\n",
    "2:13: sum requires a list of numbers, got i64; add other values in a loop"
)]
fn sum_rejects_other_arguments(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(rejection(body), expected);
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
#[case::abs_of_the_minimum("    low = i64.MIN\n    print(abs(low))?\n", "3:11: integer overflow")]
#[case::abs_of_a_narrow_minimum(
    "    low = i8.MIN\n    print(abs(low))?\n",
    "3:11: integer overflow"
)]
#[case::pow("    n = 2\n    print(1 + n.pow(63))?\n", "3:15: integer overflow")]
#[case::unsigned_pow("    n = 16u8\n    print(n.pow(2))?\n", "3:11: integer overflow")]
#[case::pow_below_the_minimum("    n = -2i8\n    print(n.pow(8))?\n", "3:11: integer overflow")]
#[case::negative_exponent(
    "    n = 2\n    print(n.pow(0 - 1))?\n",
    "3:11: negative integer exponent"
)]
#[case::sum(
    "    xs = [i64.MAX, 1]?\n    print(sum(xs))?\n",
    "3:11: integer overflow"
)]
#[case::unsigned_sum(
    "    xs = [200u8, 100u8]?\n    print(sum(xs))?\n",
    "3:11: integer overflow"
)]
#[case::sum_adds_in_order(
    "    xs = [100i8, 100i8, -100i8]?\n    print(sum(xs))?\n",
    "3:11: integer overflow"
)]
fn integer_results_outside_the_type_trap(#[case] body: &str, #[case] expected: &str) {
    assert_eq!(trap(body), expected);
}

#[test]
fn locals_and_class_methods_may_reuse_the_names() {
    assert_lines(
        "class Account:\n    total: i64\n    def sum(self) -> i64:\n        self.total\n    def pow(self, n: i64) -> i64:\n        self.total * n\n    def sqrt(self) -> i64:\n        0\n",
        "    account = Account(4)\n    sum = 10\n    min = 1\n    max = sum + min\n    abs = max\n",
        &[
            ("account.sum()", "4"),
            ("account.pow(3)", "12"),
            ("account.sqrt()", "0"),
            ("abs", "11"),
        ],
    );
}

#[rstest]
#[case("abs")]
#[case("min")]
#[case("max")]
#[case("sum")]
fn top_level_definitions_cannot_take_a_builtin_name(#[case] name: &str) {
    let error = support::check_source(&format!(
        "def {name}(x: i64) -> i64:\n    x\ndef main() -> ():\n    pass\n"
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot redefine a builtin"), "{error}");
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

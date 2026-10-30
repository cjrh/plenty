//! `NonZero[T]` (issue #584): an integer that cannot be zero, required for a
//! range step so that a zero step is unrepresentable instead of a trap.
//! See book/src/design/language/03-types/numbers.md#nonzero-integers.
mod support;
use rstest::rstest;
use support::{assert_stdout as runs, check_error};

#[test]
fn literals_convert_where_expected() {
    runs(
        "def scale(n: i64, by: NonZero[i64]) -> i64:\n    n * by.get()\n\
         a: NonZero[i64] = 3\n\
         b: NonZero[u8] = 200\n\
         c: NonZero[i32] = -7\n\
         print(a, b, c).unwrap()\n\
         print(scale(5, -2)).unwrap()\n",
        "3 200 -7\n-10\n",
    );
}

#[test]
fn literal_and_negative_steps() {
    runs(
        "print(list(range(0, 10, 3)).unwrap()).unwrap()\n\
         print(list(range(5, 0, -2)).unwrap()).unwrap()\n\
         print(list(range[u8](9, 0, -4)).unwrap()).unwrap()\n\
         print([n for n in reversed(range(0, 6, 2))].unwrap()).unwrap()\n\
         step: NonZero[i64] = -3\n\
         print(len(range(9, 0, step))).unwrap()\n",
        "[0, 3, 6, 9]\n[5, 3, 1]\n[9, 5, 1]\n[4, 2, 0]\n3\n",
    );
}

#[test]
fn try_from_converts_a_runtime_value() {
    runs(
        "def step(n: i64) -> Result[NonZero[i64], RangeError]:\n    NonZero.try_from(n)\n\
         def evens(stop: i64, stride: i64) -> Result[list[i64], AllocError | RangeError]:\n    Ok(list(range(0, stop, NonZero.try_from(stride)?))?)\n\
         print(step(4).unwrap()).unwrap()\n\
         print(step(0).is_err()).unwrap()\n\
         print(evens(7, 2).unwrap()).unwrap()\n\
         print(evens(7, 0).is_err()).unwrap()\n\
         narrow: i32 = -5\n\
         print(NonZero[i32].try_from(narrow).unwrap()).unwrap()\n\
         match NonZero.try_from(0u8):\n    case Ok(n):\n        print(n).unwrap()\n    case Err(RangeError.OutOfRange):\n        print(\"zero\").unwrap()\n",
        "4\nTrue\n[0, 2, 4, 6]\nTrue\n-5\nzero\n",
    );
}

#[test]
fn operations_keep_or_drop_the_guarantee() {
    runs(
        "a: NonZero[i64] = 6\n\
         b: NonZero[i64] = -2\n\
         product: NonZero[i64] = a * b\n\
         negated: NonZero[i64] = -a\n\
         total: i64 = a + b\n\
         difference: i64 = a - a\n\
         print(product, negated, total, difference).unwrap()\n\
         print(a == 6, a != b, b < a, a >= 6, a * 2, -2 * a).unwrap()\n\
         mut c = a\n\
         c *= b\n\
         c *= 3\n\
         print(c, c.get() // 4).unwrap()\n",
        "-12 -6 4 0\nTrue True True True 12 -12\n-36 -9\n",
    );
}

#[test]
fn checked_product_reports_overflow() {
    runs(
        "big: NonZero[i8] = 100\n\
         two: NonZero[i8] = 2\n\
         result = checked:\n    big * two\n\
         print(result.is_err()).unwrap()\n",
        "True\n",
    );
}

#[test]
fn values_print_format_and_key_collections() {
    runs(
        "import std.fmt\n\
         def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:\n\
         \x20   a: NonZero[i64] = 42\n\
         \x20   print(f\"[{a:>5}] {a:x} {a:+} {a!r}\"?)?\n\
         \x20   print(str.from(a)?)?\n\
         \x20   names = {a: \"answer\"}?\n\
         \x20   print(names)?\n\
         \x20   print(names[a])?\n\
         \x20   seen = {a}?\n\
         \x20   print(a in seen)?\n\
         \x20   steps: list[NonZero[i64]] = [1, -1]?\n\
         \x20   print(steps)?\n\
         \x20   Ok(())\n",
        "[   42] 2a +42 42\n42\n{42: \"answer\"}\nanswer\nTrue\n[1, -1]\n",
    );
}

#[test]
fn copy_classes_hold_nonzero_fields() {
    runs(
        "@copy\nclass Stride:\n    by: NonZero[i64]\n\
         s = Stride(by=-2)\n\
         t = s\n\
         print(s.by, t.by).unwrap()\n",
        "-2 -2\n",
    );
}

#[rstest]
#[case::literal_zero(
    "a: NonZero[i64] = 0",
    "`0` is not a `NonZero[i64]`: a NonZero value cannot be zero"
)]
#[case::negative_zero("a: NonZero[i64] = -0", "a NonZero value cannot be zero")]
#[case::zero_step("print(range(0, 4, 0)).unwrap()", "a NonZero value cannot be zero")]
#[case::zero_operand(
    "a: NonZero[i64] = 2\nprint(a * 0).unwrap()",
    "a NonZero value cannot be zero"
)]
#[case::zero_comparison(
    "a: NonZero[i64] = 2\nprint(a == 0).unwrap()",
    "a NonZero value cannot be zero"
)]
#[case::out_of_range("a: NonZero[u8] = 300", "out of range")]
#[case::float_element("a: NonZero[f64] = 1", "f64 does not satisfy IntType")]
#[case::plain_value(
    "n = 3\na: NonZero[i64] = n",
    "convert it with `NonZero.try_from(value)?`, which rejects zero"
)]
#[case::i64_step(
    "n = 3\nprint(range(0, 9, n)).unwrap()",
    "range step must be `NonZero[i64]`, not `i64`; write a nonzero literal, or convert a value with `NonZero.try_from(step)?`"
)]
#[case::constructor(
    "a = NonZero(3)",
    "a `NonZero` comes from a nonzero integer literal where one is expected, or from `NonZero.try_from(value)?`"
)]
#[case::field(
    "a: NonZero[i64] = 3\nprint(a.value).unwrap()",
    "a `NonZero`'s field is private; read its integer with `.get()`"
)]
#[case::floor_division(
    "a: NonZero[i64] = 3\nprint(a // a).unwrap()",
    "operator `//` does not accept `NonZero[i64]`, since its result could be zero; read the integer with `.get()`"
)]
#[case::remainder(
    "a: NonZero[i64] = 3\nprint(a % a).unwrap()",
    "since its result could be zero"
)]
#[case::invert(
    "a: NonZero[i64] = 3\nprint(~a).unwrap()",
    "since its result could be zero"
)]
#[case::add_assign("mut a: NonZero[i64] = 3\na += a", "since its result could be zero")]
#[case::try_from_width(
    "n: i32 = 3\nprint(NonZero[i64].try_from(n)).unwrap()",
    "`NonZero[i64].try_from` takes `i64`, not `i32`; convert it first with `i64.try_from(value)?`"
)]
#[case::try_from_float(
    "print(NonZero.try_from(1.5)).unwrap()",
    "NonZero.try_from takes an integer, not f64"
)]
#[case::redefine("class NonZero:\n    x: i64", "NonZero")]
fn rejected(#[case] source: &str, #[case] diagnostic: &str) {
    let error = check_error(source);
    assert!(
        error.contains(diagnostic),
        "{source}\nexpected {diagnostic:?}, got {error:?}"
    );
}

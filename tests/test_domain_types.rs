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
print(sum, product, difference, negative, n, p).unwrap()
print(a == 3, a > b, n >= 0).unwrap()
overflow = checked:
    a * 100
print(overflow.is_err()).unwrap()
addition = checked:
    p + 120
print(addition.is_err()).unwrap()
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
def negate(value: &Positive[i8]) -> i8:
    -value
def increment(value: &NonNegative[i8]) -> NonNegative[i8]:
    value + 1
def overflow(value: Positive[i8]) -> Result[Positive[i8], OverflowError[i8]]:
    checked:
        value * 2
p: Positive[i8] = 100
n: NonNegative[i8] = 0
print(negate(&p), increment(&n)).unwrap()
match overflow(p):
    case Ok(_):
        print("unexpected").unwrap()
    case Err(OverflowError(wrapped)):
        print(wrapped).unwrap()
"#,
        "-100 1\n-56\n",
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
#[case("p: Positive[i64] = 3\nx = 2\nprint(p + x).unwrap()", "only between two")]
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

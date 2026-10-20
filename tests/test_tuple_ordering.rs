mod support;
use support::assert_stdout as native;

#[test]
fn tuple_ordering_is_lexicographic_for_all_four_operators() {
    native(
        r#"
a = (1, ("alpha", -4))
b = (1, ("beta", -9))
print(a < b, a <= b, a > b, a >= b).unwrap()
print(b < a, b <= a, b > a, b >= a).unwrap()
print(a < a, a <= a, a > a, a >= a).unwrap()
print((0, "z") < (1, "a"), (7,) >= (7,)).unwrap()
print((18446744073709551615u64, -128i8) > (0u64, 127i8)).unwrap()
print((f32(1.5),) < (f32(2.0),)).unwrap()
print(a, b).unwrap()
"#,
        "True True False False\nFalse False True True\nFalse True False True\nTrue True\nTrue\nTrue\n(1, (\"alpha\", -4)) (1, (\"beta\", -9))\n",
    );
}

#[test]
fn borrowed_tuple_operators_work_in_generic_functions() {
    native(
        r#"
def before[T](a: &T, b: &T) -> bool:
    *a < *b
a = (3, ("orchard-" + "pears").unwrap())
b = (3, ("orchard-" + "plums").unwrap())
print(before[(i64, str)](&a, &b), &a <= &b).unwrap()
print(a, b).unwrap()
"#,
        "True True\n(3, \"orchard-pears\") (3, \"orchard-plums\")\n",
    );
}

#[test]
fn tuple_operands_evaluate_once_from_left_to_right() {
    native(
        r#"
def operand(n: i64) -> (i64, str):
    print(n).unwrap()
    (n, ("temporary-" + "value").unwrap())
print(operand(1) < operand(2)).unwrap()
"#,
        "1\n2\nTrue\n",
    );
}

#[test]
fn tuple_float_order_matches_sort_including_nan_and_signed_zero() {
    native(
        r#"
nan = 0.0 / 0.0
print((-0.0,) < (0.0,), (-0.0,) == (0.0,)).unwrap()
print((nan,) > (1.0,), (nan,) >= (nan,), (nan,) == (nan,)).unwrap()
print(nan > 1.0, nan >= nan, -0.0 < 0.0).unwrap()
mut values = [(nan, 2), (0.0, 0), (-0.0, 9), (-2.0, 8), (nan, 1)].unwrap()
values.sort()
for i in range(1, len(values)):
    print(values[i - 1] < values[i], values[i - 1] <= values[i]).unwrap()
print(values).unwrap()
"#,
        "True True\nTrue True False\nFalse False False\nTrue True\nTrue True\nTrue True\nTrue True\n[(-2.0, 8), (-0.0, 9), (0.0, 0), (NaN, 1), (NaN, 2)]\n",
    );
}

#[test]
fn tuple_ordering_rejects_unordered_fields_and_mismatched_types() {
    for expression in [
        "(1, True) < (1, False)",
        "(1, [2].unwrap()) < (1, [3].unwrap())",
        "(1, (False,)) <= (1, (True,))",
        "(1, ()) > (1, ())",
        "() < ()",
        "(1,) < (1, 2)",
        "(i8(1),) < (i16(2),)",
        "(1, 2) + (3, 4)",
    ] {
        assert!(support::check_source(expression).is_err(), "{expression}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn comparing_owned_tuple_strings_allocates_nothing() {
    let output = support::run(
        r#"
a = (("orchard-" + "pears").unwrap(), (3, "a"))
b = (("orchard-" + "plums").unwrap(), (1, "z"))
print("__test_fail_allocations_after_0__").unwrap()
less = a < b
at_most = &a <= &b
greater = a > b
at_least = &a >= &b
print("__test_restore_allocations__").unwrap()
print(less, at_most, greater, at_least).unwrap()
print(a, b).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let visible = text
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        [
            "True True False False",
            "(\"orchard-pears\", (3, \"a\")) (\"orchard-plums\", (1, \"z\"))"
        ]
    );
}

#[test]
fn flat_and_nested_tuple_sorts_match_tuple_operators() {
    native(
        r#"
def ordered(rows: &list[(u8, str, f64, i32)], descending: bool) -> bool:
    for i in range(1, len(rows)):
        if (not descending and rows[i - 1] > rows[i]) or (descending and rows[i - 1] < rows[i]):
            return False
    True
mut rows: list[(u8, str, f64, i32)] = [].unwrap()
mut nested: list[(i64, (str, i64))] = [].unwrap()
mut s = 11
for i in range(2000):
    s = (s * 1103515245 + 12345) % 2147483648
    mut score = f64(s % 13) - 6.0
    if s % 31 == 0:
        score = 0.0 / 0.0
    if s % 37 == 0:
        score = -0.0
    rows.append((u8(s % 3 + 250), f"k{s % 5}".unwrap(), score, i32(s % 9) - 4)).unwrap()
    nested.append((s % 4, (f"n{s % 6}".unwrap(), i))).unwrap()
rows.sort()
print(ordered(&rows, False)).unwrap()
rows.sort(True)
print(ordered(&rows, True)).unwrap()
nested.sort()
mut fine = True
for i in range(1, len(nested)):
    if nested[i - 1] > nested[i]:
        fine = False
print(fine).unwrap()
"#,
        "True\nTrue\nTrue\n",
    );
}

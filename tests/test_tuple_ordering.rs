mod support;

/// Only the fixture entrypoint owns fragment arithmetic; declarations retain their own contracts.
fn program(source: &str) -> String {
    if source.lines().any(|line| line.starts_with("def main(")) {
        return source.to_owned();
    }
    let source = support::program(source);
    let source = source.replacen(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64] | OverflowError[i32] | ZeroDivisionError]:",
        1,
    );
    let end = source.rfind("    pass\n").unwrap();
    format!("{}    Ok(())\n", &source[..end])
}

fn native(source: &str, expected: &str) {
    support::assert_stdout(&program(source), expected);
}

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

/// Tuple operators compare components with their own operators, as Rust's
/// tuple `PartialOrd` does: signed zeros are equal, and the first unordered
/// component makes every relation false (issue #616).
#[test]
fn tuple_float_operators_follow_scalar_ieee_comparison() {
    native(
        r#"
def relations[T](a: T, b: T) -> (bool, bool, bool, bool, bool, bool):
    (a < b, a <= b, a > b, a >= b, a == b, a != b)
nan = 0.0 / 0.0
inf = f64.INF
print(nan <= nan, (nan,) <= (nan,), (nan,) == (nan,)).unwrap()
print(relations((-0.0,), (0.0,))).unwrap()
print(relations((nan, 0), (nan, 1))).unwrap()
print(relations((0, nan), (1, nan))).unwrap()
print(relations((1, nan), (1, 2.0))).unwrap()
print(relations((1, 2.0), (1, nan))).unwrap()
print(relations((nan,), (inf,))).unwrap()
print(relations((-inf, "b"), (inf, "a"))).unwrap()
print(relations((inf, "b"), (inf, "a"))).unwrap()
print(relations(((1, nan), 2), ((1, nan), 3))).unwrap()
print(relations(((0, nan), 2), ((1, nan), 3))).unwrap()
print(relations((f32(-0.0), 1), (f32(0.0), 2))).unwrap()
print((nan, 1) in [(nan, 1)].unwrap(), (-0.0, 1) in [(0.0, 1)].unwrap()).unwrap()
"#,
        "False False False\n\
         (False, True, False, True, True, False)\n\
         (False, False, False, False, False, True)\n\
         (True, True, False, False, False, True)\n\
         (False, False, False, False, False, True)\n\
         (False, False, False, False, False, True)\n\
         (False, False, False, False, False, True)\n\
         (True, True, False, False, False, True)\n\
         (False, False, True, True, False, True)\n\
         (False, False, False, False, False, True)\n\
         (True, True, False, False, False, True)\n\
         (True, True, False, False, False, True)\n\
         False True\n",
    );
}

/// Sorting keeps its separate total order: zeros by sign, NaNs last.
#[test]
fn tuple_sorting_keeps_its_total_order() {
    native(
        r#"
nan = 0.0 / 0.0
mut values = [(nan, 2), (0.0, 0), (-0.0, 9), (-2.0, 8), (nan, 1)].unwrap()
values.sort()
print(values).unwrap()
# The sort placed -0.0 first, but the operators see equal zeros and compare
# the second field, so they report this sorted pair reversed.
print(values[1] > values[2], values[3] < values[4]).unwrap()
values.sort(True)
print(values).unwrap()
mut keyed = [3, 1, 2].unwrap()
keyed.sort_by_key(def(x: &i64) -> (f64, i64): (f64.NAN if *x == 2 else -0.0 if *x == 1 else 0.0, *x))
print(keyed).unwrap()
"#,
        "[(-2.0, 8), (-0.0, 9), (0.0, 0), (NaN, 1), (NaN, 2)]\nTrue False\n[(NaN, 2), (NaN, 1), (0.0, 0), (-0.0, 9), (-2.0, 8)]\n[1, 3, 2]\n",
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

/// Sorting agrees with the operators except where a signed zero decides the
/// sort, so the rows avoid `-0.0`; a NaN component leaves a pair unordered,
/// never out of order.
#[test]
fn flat_and_nested_tuple_sorts_match_tuple_operators() {
    native(
        r#"
def ordered(rows: &list[(u8, str, f64, i32)], descending: bool) -> Result[bool, OverflowError[i64]]:
    for i in range(1, len(rows)):
        if (not descending and rows[i - 1] > rows[i]) or (descending and rows[i - 1] < rows[i]):
            return Ok(False)
    Ok(True)
mut rows: list[(u8, str, f64, i32)] = [].unwrap()
mut nested: list[(i64, (str, i64))] = [].unwrap()
mut s = 11
for i in range(2000):
    s = (s * 1103515245 + 12345) % 2147483648
    mut score = f64(s % 13) - 6.0
    if s % 31 == 0:
        score = 0.0 / 0.0
    rows.append((u8(s % 3 + 250), f"k{s % 5}".unwrap(), score, i32(s % 9) - 4)).unwrap()
    nested.append((s % 4, (f"n{s % 6}".unwrap(), i))).unwrap()
rows.sort()
print(ordered(&rows, False)?).unwrap()
rows.sort(True)
print(ordered(&rows, True)?).unwrap()
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

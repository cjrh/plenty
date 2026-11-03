//! Quantity core: storage arithmetic remains ordinary numeric arithmetic.
mod support;
use support::assert_stdout;

const BASES: &str = "dimension Length(\"m\")\ndimension Time(\"s\")\n";

fn rejects(body: &str, message: &str) {
    let source = format!("{BASES}def main() -> ():\n{body}\n    pass\n");
    let error = plenty::check_source(&source).unwrap_err().to_string();
    assert!(
        error.contains(message),
        "{source}\n{error}\nexpected {message}"
    );
}

#[test]
fn float_arithmetic_dimensions_scalars_and_comparisons() {
    for t in ["f32", "f64"] {
        assert_stdout(
            &format!(
                "{BASES}
def main() -> Result[(), IoError]:
    a = Quantity[{t}, Length](12.0)
    b = Quantity[{t}, Time](3.0)
    sum: Quantity[{t}, Length] = a + a
    difference: Quantity[{t}, Length] = a - a
    product: Quantity[{t}, Length * Time] = a * b
    velocity: Quantity[{t}, Length / Time] = a / b
    ratio: Quantity[{t}, One] = a / a
    scaled: Quantity[{t}, Length] = 2.0 * a + a * 2.0
    divided: Quantity[{t}, Length] = a / 2.0
    inverse: Quantity[{t}, One / Length] = 24.0 / a
    negative: Quantity[{t}, Length] = -a
    positive: Quantity[{t}, Length] = +a
    print(sum.get(), difference.get(), product.get(), velocity.get(), ratio.get())?
    print(scaled.get(), divided.get(), inverse.get(), negative.get(), positive.get())?
    print(a == a, a != a, a < a, a <= a, a > a, a >= a)?
    mut q = a
    q += a
    q -= a
    q *= 2.0
    q /= 4.0
    print(q.get())?
    Ok(())
"
            ),
            "24.0 0.0 36.0 4.0 1.0\n48.0 6.0 2.0 -12.0 12.0\nTrue False False True False True\n6.0\n",
        );
    }
}

#[test]
fn every_integer_storage_keeps_dimensions() {
    for t in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        assert_stdout(&format!("{BASES}
def main() -> Result[(), IoError]:
    a = Quantity[{t}, Length](12)
    b = Quantity[{t}, Time](5)
    sum: Quantity[{t}, Length] = a + a
    difference: Quantity[{t}, Length] = a - a
    product: Quantity[{t}, Length * Time] = a * b
    quotient: Quantity[{t}, Length / Time] = a // b
    remainder: Quantity[{t}, Length] = a % Quantity[{t}, Length](5)
    scaled: Quantity[{t}, Length] = 2 * a + a * 2
    positive: Quantity[{t}, Length] = +a
    print(sum.get(), difference.get(), product.get(), quotient.get(), remainder.get(), scaled.get(), positive.get())?
    print(a == a, a != a, a < a, a <= a, a > a, a >= a)?
    mut q = a
    q += a
    q -= a
    q *= 2
    print(q.get())?
    Ok(())
"), "24 0 60 2 2 48 12\nTrue False False True False True\n24\n");
    }
}

#[test]
fn domain_results_keep_the_quantity_wrapper() {
    for (domain, widths) in [
        ("Positive", &["i8", "i16", "i32", "i64"][..]),
        ("NonNegative", &["i8", "i16", "i32", "i64"][..]),
        (
            "NonZero",
            &["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"][..],
        ),
    ] {
        for t in widths {
            let sum = if domain == "NonZero" {
                t.to_string()
            } else {
                format!("{domain}[{t}]")
            };
            let sum_get = if domain == "NonZero" {
                "sum.get()"
            } else {
                "sum.get().get()"
            };
            assert_stdout(
                &format!(
                    "{BASES}
def main() -> Result[(), IoError]:
    p = Quantity[{domain}[{t}], Length](3)
    sum: Quantity[{sum}, Length] = p + p
    difference: Quantity[{t}, Length] = p - p
    product: Quantity[{domain}[{t}], Pow[Length, 2]] = p * p
    print({sum_get}, difference.get(), product.get().get())?
    Ok(())
"
                ),
                "6 0 9\n",
            );
        }
    }
}

#[test]
fn dimensions_and_storage_rejections() {
    for (body, message) in [
        (
            "    q = Quantity[f64, Length](1.0) + Quantity[f64, Time](1.0)",
            "requires equal dimensions",
        ),
        (
            "    q = Quantity[f64, Length](1.0) == Quantity[f64, Time](1.0)",
            "requires equal dimensions",
        ),
        (
            "    q = Quantity[f64, Length](1.0) + 0.0",
            "scalars only scale",
        ),
        (
            "    q = Quantity[f64, Length](1.0) > 0.0",
            "scalars only scale",
        ),
        (
            "    q = Quantity[f32, Length](1.0) + Quantity[f64, Length](1.0)",
            "f32",
        ),
        (
            "    q: f64 = Quantity[f64, Length](1.0) / Quantity[f64, Length](1.0)",
            "Quantity",
        ),
        ("    q = Quantity[str, Length](\"bad\")", "Quantity storage"),
        (
            "    q = Quantity[Positive[i64], Length](0)",
            "greater than zero",
        ),
        (
            "    q = Quantity[i64, Length](1) & Quantity[i64, Length](1)",
            "do not support",
        ),
        ("    q = Quantity[i64, Length](1).value", "private"),
        (
            "    mut q = Quantity[f64, Length](1.0)\n    q *= q",
            "require a scalar",
        ),
        (
            "    mut q = Quantity[f64, Length](1.0)\n    q += Quantity[f64, Time](1.0)",
            "equal dimensions",
        ),
    ] {
        rejects(body, message);
    }
}

#[test]
fn generic_results_evaluate_once_in_source_order() {
    assert_stdout(
        &format!(
            "{BASES}
def make[D: Dimension](n: f64, order: &mut i64) -> Quantity[f64, D]:
    *order = *order * 10 + i64(n)
    Quantity[f64, D](n)
def main() -> Result[(), IoError]:
    mut order = 0
    area: Quantity[f64, Length * Time] = make[Length](2.0, &mut order) * make[Time](3.0, &mut order)
    sum: Quantity[f64, Length] = make[Length](4.0, &mut order) + make[Length](5.0, &mut order)
    print(area.get(), sum.get(), order)?
    Ok(())
"
        ),
        "6.0 9.0 2345\n",
    );
}

#[test]
fn conversions_and_coherent_units() {
    assert_stdout(
        &format!(
            "{BASES}
def main() -> Result[(), IoError | RangeError]:
    metre = Quantity[f64, Length](1.0)
    km = 1000.0 * metre
    distance = 2500.0 * metre
    narrow: Quantity[f32, Length] = Quantity[f32, Length].from(distance)
    wide: Quantity[f64, Length] = Quantity[f64, Length].from(narrow)
    p: Quantity[Positive[i64], Length] = Quantity[Positive[i64], Length].try_from(3)?
    rejected = Quantity[Positive[i64], Length].try_from(0)
    print(distance.to(km), narrow.get(), wide.get(), p.get().get(), rejected.is_err())?
    Ok(())
"
        ),
        "2.5 2500.0 2500.0 3 True\n",
    );
    rejects(
        "    q = Quantity[f32, Length].from(Quantity[f64, Time](1.0))",
        "equal dimensions",
    );
    rejects(
        "    q = Quantity[f64, Length](1.0).to(Quantity[f64, Time](1.0))",
        "Quantity",
    );
}

#[test]
fn compound_destinations_and_inferred_rhs_are_evaluated_once() {
    assert_stdout(
        &format!(
            "{BASES}
def identity[T](x: T) -> T:
    x
def index(order: &mut i64) -> i64:
    *order = *order * 10 + 1
    0
def destination(q: &mut Quantity[f64, Length]) -> &mut Quantity[f64, Length]:
    q
def addend(order: &mut i64) -> Quantity[f64, Length]:
    *order = *order * 10 + 2
    Quantity[f64, Length](3.0)
def main() -> Result[(), AllocError | IoError]:
    mut order = 0
    mut values = [Quantity[f64, Length](1.0)]?
    values[index(&mut order)] += identity(addend(&mut order))
    value = &mut values[0]
    *value *= 2.0
    *destination(value) += Quantity[f64, Length](1.0)
    print(value.get(), order)?
    Ok(())
"
        ),
        "9.0 12\n",
    );
}

#[test]
fn checked_errors_and_proven_divisors_keep_numeric_contracts() {
    assert_stdout(&format!("{BASES}
def overflow() -> Result[Quantity[i8, Length], OverflowError[i8]]:
    checked:
        Quantity[i8, Length](127) + Quantity[i8, Length](1)
def division() -> Result[Quantity[i64, Length / Time], ZeroDivisionError | OverflowError[i64]]:
    checked:
        Quantity[i64, Length](12) // Quantity[i64, Time](0)
def positive_divisor() -> Quantity[i64, Length / Time]:
    Quantity[i64, Length](-7) // Quantity[Positive[i64], Time](3)
def nonzero_divisor() -> Result[Quantity[i64, Length / Time], OverflowError[i64]]:
    checked:
        Quantity[i64, Length](i64.MIN) // Quantity[NonZero[i64], Time](-1)
def main() -> Result[(), IoError]:
    print(overflow().is_err(), division().is_err(), nonzero_divisor().is_err(), positive_divisor().get())?
    p = Quantity[Positive[i64], Length](3)
    n: Quantity[i64, Length] = -p
    mut q = Quantity[Positive[i64], Length](3)
    q += p
    q *= 2
    print(n.get(), q.get().get())?
    Ok(())
"), "True True True -3\n-3 12\n");
}

#[test]
fn arithmetic_and_unit_conversion_do_not_allocate() {
    assert_stdout(
        &format!(
            "{BASES}
def main() -> Result[(), IoError]:
    metre = Quantity[f64, Length](1.0)
    km = 1000.0 * metre
    print(\"__test_begin_no_allocations__\")?
    print(\"__test_fail_allocations_after_0__\")?
    mut q = 500.0 * metre + 1500.0 * metre
    q *= 2.0
    value = q.to(km)
    print(\"__test_restore_allocations__\")?
    print(\"__test_end_no_allocations__\")?
    print(value)?
    Ok(())
"
        ),
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n4.0\n",
    );
}

#[test]
fn quantity_updates_preserve_borrow_and_domain_invariants() {
    for body in [
        "    q = Quantity[f64, Length](1.0)\n    q += q",
        "    mut q = Quantity[f64, Length](1.0)\n    borrowed = &q\n    q += q\n    drop(borrowed.get())",
        "    mut q = Quantity[Positive[i64], Length](1)\n    q -= q",
        "    q: Positive[Quantity[i64, Length]] = 1",
        "    q = Quantity[f64, Pow[Length, 127]](1.0) * Quantity[f64, Length](1.0)",
    ] {
        let source = format!("{BASES}def main() -> ():\n{body}\n    pass\n");
        assert!(plenty::check_source(&source).is_err(), "{source}");
    }
    let source = format!(
        "{BASES}
def mutate(q: &mut Quantity[f64, Length]) -> Quantity[f64, Length]:
    *q = Quantity[f64, Length](2.0)
    Quantity[f64, Length](1.0)
def main() -> ():
    mut q = Quantity[f64, Length](1.0)
    q += mutate(&mut q)
"
    );
    assert!(plenty::check_source(&source).is_err(), "{source}");
}

#[test]
fn computed_quantities_support_every_ordering() {
    for (operator, expected) in [
        ("<", "True"),
        ("<=", "True"),
        (">", "False"),
        (">=", "False"),
    ] {
        assert_stdout(
            &format!(
                "{BASES}
def identity[T](value: T) -> T:
    value
def main() -> Result[(), IoError]:
    q = Quantity[f64, Length](1.0)
    print((q + q) {operator} (q * 3.0))?
    print(identity(q + q) {operator} identity(q * 3.0))?
    p = Quantity[Positive[i32], Length](1)
    print((p + p) {operator} (p * 3))?
    print((p - p) {operator} (p - p))?
    Ok(())
"
            ),
            &format!(
                "{expected}\n{expected}\n{expected}\n{}\n",
                if matches!(operator, "<=" | ">=") {
                    "True"
                } else {
                    "False"
                }
            ),
        );
        rejects(
            &format!(
                "    q = Quantity[f64, Length](1.0)\n    t = Quantity[f64, Time](1.0)\n    compared = (q + q) {operator} (t + t)"
            ),
            "requires equal dimensions",
        );
    }
}

#[test]
fn computed_comparisons_evaluate_once_and_keep_numeric_context() {
    assert_stdout(
        &format!(
            "{BASES}
def identity[T](value: T) -> T:
    value
def traced[T](value: T, digit: i64, order: &mut i64) -> T:
    *order = *order * 10 + digit
    value
def main() -> Result[(), IoError]:
    mut order = 0
    q = Quantity[f64, Length](1.0)
    compared = traced(q + q, 1, &mut order) < traced(q * 3.0, 2, &mut order)
    print(compared, order)?
    # The RHS unsuffixed literal still gets the actual left storage type.
    print(identity(2i32) < (3 if True else 4))?
    print(identity(2.0f32) < (3.0 if True else 4.0))?
    print(identity(2i32) < 3, 2 < 3i32)?
    Ok(())
"
        ),
        "True 12\nTrue\nTrue\nTrue True\n",
    );
}

#[test]
fn quantity_ordering_with_slice_views_reports_a_diagnostic() {
    for expression in ["q < &xs[:]", "(q + q) < &xs[:]", "&xs[:] < (q + q)"] {
        rejects(
            &format!(
                "    q = Quantity[f64, Length](1.0)\n    xs = [1.0].unwrap()\n    compared = {expression}"
            ),
            "slice views support `==` and `!=`, not `<`",
        );
    }
}

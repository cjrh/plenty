//! Quantity module constants retain storage and dimensions during evaluation.
mod support;
use support::assert_stdout;

#[test]
fn units_and_derived_values_fold_at_compile_time() {
    assert_stdout(
        r#"
dimension Length("m")
dimension Time("s")
const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)
const second: Quantity[f64, Time] = Quantity[f64, Time](1.0)
const km: Quantity[f64, Length] = 1000.0 * metre
const minute: Quantity[f64, Time] = second * 60.0
const speed: Quantity[f64, Length / Time] = km / minute
const area: Quantity[f64, Pow[Length, 2]] = km * km
const recovered: Quantity[f64, Length] = area / km
const distance: f64 = recovered.to(metre)
const ratio: Quantity[f64, One] = km / metre
assert distance == 1000.0
assert ratio.get() == 1000.0
assert (km + km) > (km * 1.5)
def main() -> Result[(), IoError]:
    print(km, minute, speed, recovered, ratio)?
    print(distance, area.get())?
    Ok(())
"#,
        "1000.0 m 60.0 s 16.666666666666668 m/s 1000.0 m 1000.0\n1000.0 1000000.0\n",
    );
}

#[test]
fn domain_storage_preserves_and_erases_domains_like_runtime() {
    assert_stdout(
        r#"
dimension Length("m")
const positive: Positive[i64] = 3
const p: Quantity[Positive[i64], Length] = Quantity[Positive[i64], Length](positive)
const twice: Quantity[Positive[i64], Length] = p + p
const square: Quantity[Positive[i64], Pow[Length, 2]] = p * p
const zero: Quantity[i64, Length] = p - p
const negative: Quantity[i64, Length] = -p
const amount: i64 = twice.get().get()
assert amount == 6
assert zero.get() == 0
assert twice > p
def main() -> Result[(), IoError]:
    print(twice, square, zero, negative, amount)?
    Ok(())
"#,
        "6 m 9 m^2 0 m -3 m 6\n",
    );
}

#[test]
fn integer_widths_and_domains_match_ordinary_quantity_operators() {
    for width in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        assert_stdout(
            &format!(
                r#"
dimension L("m")
dimension T("s")
const a: Quantity[{width}, L] = Quantity[{width}, L](12)
const b: Quantity[{width}, T] = Quantity[{width}, T](5)
const c: Quantity[{width}, L] = Quantity[{width}, L](5)
const sum: Quantity[{width}, L] = a + a
const difference: Quantity[{width}, L] = a - a
const product: Quantity[{width}, L * T] = a * b
const quotient: Quantity[{width}, L / T] = a // b
const remainder: Quantity[{width}, L] = a % c
const scaled: Quantity[{width}, L] = 2 * a + a * 2
const positive: Quantity[{width}, L] = +a
const divisor: Quantity[NonZero[{width}], T] = Quantity[NonZero[{width}], T](5)
const proven: Quantity[{width}, L / T] = a // divisor
def main() -> Result[(), IoError | OverflowError[{width}] | ZeroDivisionError]:
    print(sum == a + a, difference == a - a, product == a * b, quotient == a // b)?
    print(remainder == a % c, scaled == 2 * a + a * 2, positive == +a, proven == a // divisor)?
    print(sum.get(), difference.get(), product.get(), quotient.get(), remainder.get(), scaled.get())?
    Ok(())
"#
            ),
            "True True True True\nTrue True True True\n24 0 60 2 2 48\n",
        );
        for domain in ["Positive", "NonNegative", "NonZero"] {
            if width.starts_with('u') && domain != "NonZero" {
                continue;
            }
            let sum = if domain == "NonZero" {
                width.to_string()
            } else {
                format!("{domain}[{width}]")
            };
            assert_stdout(
                &format!(
                    r#"
dimension L("m")
const q: Quantity[{domain}[{width}], L] = Quantity[{domain}[{width}], L](3)
const doubled: Quantity[{sum}, L] = q + q
const square: Quantity[{domain}[{width}], Pow[L, 2]] = q * q
const difference: Quantity[{width}, L] = q - q
const scaled: Quantity[{domain}[{width}], L] = 2 * q
const amount: {width} = square.get().get()
def main() -> Result[(), IoError | OverflowError[{width}]]:
    print(doubled == q + q, square == q * q, difference == q - q, scaled == 2 * q)?
    print(amount)?
    Ok(())
"#
                ),
                "True True True True\n9\n",
            );
        }
    }
}

#[test]
fn float_rounding_signed_zero_nan_and_casts_match_runtime() {
    for width in ["f32", "f64"] {
        assert_stdout(
            &format!(
                r#"
dimension L("m")
const a: Quantity[{width}, L] = Quantity[{width}, L](0.1)
const b: Quantity[{width}, L] = Quantity[{width}, L](0.2)
const sum: Quantity[{width}, L] = a + b
const negative: Quantity[{width}, L] = -Quantity[{width}, L](0.0)
const inverse: Quantity[{width}, One / L] = 1.0 / a
const nan: Quantity[{width}, L] = Quantity[{width}, L]({width}.NAN)
const narrow: Quantity[f32, L] = Quantity[f32, L](f32(sum.get()))
assert nan != nan
assert not (nan < a)
def main() -> Result[(), IoError]:
    print(sum == a + b, inverse == 1.0 / a, narrow == Quantity[f32, L].from(sum))?
    print(negative)?
    Ok(())
"#
            ),
            "True True True\n-0.0 m\n",
        );
    }
}

#[test]
fn unselected_arithmetic_is_not_evaluated_but_types_are_checked() {
    assert_stdout(
        r#"
dimension L("m")
const max: Quantity[i8, L] = Quantity[i8, L](i8.MAX)
const zero: Quantity[i8, L] = Quantity[i8, L](0)
const one: Quantity[i8, L] = Quantity[i8, L](1)
const selected: Quantity[i8, L] = one if True else max + one
const quotient: Quantity[i8, One] = Quantity[i8, One](1) if True else max // zero
assert True or max + one == zero
assert not (False and max // zero == quotient)
def main() -> Result[(), IoError]:
    print(selected, quotient)?
    Ok(())
"#,
        "1 m 1\n",
    );
}

#[test]
fn invalid_constants_never_lose_dimensions_or_run_user_code() {
    let cases = [
        ("const q: Quantity[f64, L] = 1.0", "expected Quantity"),
        ("const q: Quantity[f64, L] = a + t", "equal dimensions"),
        ("const q: Quantity[f64, L] = a + 1.0", "scalars only scale"),
        ("const q: bool = a > 0.0", "scalars only scale"),
        ("const q: f64 = a / a", "expected f64"),
        (
            "const q: Quantity[f64, L] = a * Quantity[f32, L](1.0)",
            "cannot combine",
        ),
        (
            "const q: Quantity[f64, L] = a if True else t",
            "conditional branches differ",
        ),
        (
            "const q: Quantity[f64, L] = Quantity[f64, L](made())",
            "not a constant expression",
        ),
        (
            "const q: Quantity[Positive[i8], L] = Quantity[Positive[i8], L](0)",
            "not a `Positive[i8]`",
        ),
        (
            "const q: Quantity[Positive[i8], L] = Quantity[Positive[i8], L](1i8 + 1i8)",
            "expected Positive",
        ),
        (
            "const q: Quantity[i8, L] = Quantity[i8, L](127) + Quantity[i8, L](1)",
            "constant arithmetic overflows",
        ),
        (
            "const q: Quantity[i8, One] = Quantity[i8, L](1) // Quantity[i8, L](0)",
            "division by zero",
        ),
        (
            "const q: Quantity[NonZero[i8], One] = p // p",
            "does not accept",
        ),
        ("const q: i64 = ~p.get()", "does not accept"),
        ("const q: f64 = f64(p.get())", "read the domain"),
        (
            "const q: Quantity[i8, Pow[L, 127]] = Quantity[i8, Pow[L, 127]](1) * Quantity[i8, L](1)",
            "exponent",
        ),
        (
            "const q: Quantity[f64, L] = later\nconst later: Quantity[f64, L] = q",
            "depends on itself",
        ),
    ];
    for (declarations, message) in cases {
        let source = format!(
            r#"
dimension L("m")
dimension T("s")
const a: Quantity[f64, L] = Quantity[f64, L](1.0)
const t: Quantity[f64, T] = Quantity[f64, T](1.0)
const p: Quantity[NonZero[i8], L] = Quantity[NonZero[i8], L](1)
def made() -> f64:
    1.0
{declarations}
def main() -> ():
    pass
"#
        );
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains(message),
            "{declarations}\n{error}\nexpected {message}"
        );
    }
}

#[test]
fn imported_constants_are_forward_resolved_public_values() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("units.plenty"),
        r#"
pub dimension Length("m")
pub const km: Quantity[f64, Length] = 1000.0 * metre
pub const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)
const hidden: Quantity[f64, Length] = km
"#,
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
import units as u
from units import km as kilometre
const distance: Quantity[f64, u.Length] = kilometre * 2.0
assert distance.to(u.metre) == 2000.0
def main() -> Result[(), IoError]:
    print(distance)?
    Ok(())
"#,
    )
    .unwrap();
    let exe = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &exe, None).unwrap();
    let output = std::process::Command::new(exe).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"2000.0 m\n");
    std::fs::write(
        &main,
        "from units import hidden\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    assert!(
        plenty::check_file(&main, None)
            .unwrap_err()
            .to_string()
            .contains("private")
    );
    std::fs::write(dir.path().join("units.plenty"),
        "dimension Hidden(\"m\")\npub const leaked: Quantity[f64, Hidden] = Quantity[f64, Hidden](1.0)\n").unwrap();
    std::fs::write(&main, "import units\ndef main() -> ():\n    pass\n").unwrap();
    assert!(
        plenty::check_file(&main, None)
            .unwrap_err()
            .to_string()
            .contains("private")
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn reading_and_using_quantity_constants_does_not_allocate() {
    assert_eq!(
        support::run_file_stdout(
            r#"
dimension Length("m")
const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)
const km: Quantity[f64, Length] = 1000.0 * metre
const positive: Quantity[Positive[i64], Length] = Quantity[Positive[i64], Length](3)
const square: Quantity[Positive[i64], Pow[Length, 2]] = positive * positive
def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    distance = km * 2.0
    count = square.get().get()
    converted = distance.to(km)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(distance, converted, count)?
    Ok(())
"#
        ),
        "2000.0 m 2.0 9",
    );
}

#[test]
fn domain_extractions_and_suffixed_scalars_keep_runtime_types() {
    assert_stdout(
        r#"
dimension L("m")
const p: Quantity[Positive[i8], L] = Quantity[Positive[i8], L](3)
const scaled: Quantity[Positive[i8], L] = 2i8 * p
const delta: Quantity[i8, L] = (p.get() - p.get()) * Quantity[i8, L](1)
const negative: Quantity[i8, L] = (-p.get()) * Quantity[i8, L](1)
const reconstructed: Quantity[Positive[i8], L] = Quantity[Positive[i8], L](p.get() + 2i8)
assert scaled == p * 2i8
def main() -> Result[(), IoError | OverflowError[i8]]:
    print(scaled == 2i8 * p, delta == (p.get() - p.get()) * Quantity[i8, L](1))?
    print(negative, reconstructed)?
    Ok(())
"#,
        "True True\n-3 m 5 m\n",
    );
}

#[test]
fn extracted_domain_constants_still_enforce_standard_library_ceilings() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
from std.fmt import string_formatter
dimension Depth("levels")
const depth: Quantity[NonNegative[i64], Depth] = Quantity[NonNegative[i64], Depth](65)
def main() -> Result[(), FormatError]:
    out = string_formatter(200, depth.get())?
    Ok(())
"#,
    )
    .unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("above its fixed ceiling of 64"), "{error}");
}

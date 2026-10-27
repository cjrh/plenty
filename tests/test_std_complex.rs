//! `std.complex` is embedded Plenty; these programs check its arithmetic,
//! IEEE edge cases, polar conversions, and display spelling.
use std::process::Command;

fn run_mode(body: &str, optimize: bool) -> String {
    run_program(
        &format!(
            "from std.complex import Complex, from_polar\nimport std.math as math\nimport std.fmt\nfrom std.fmt import FormatError\ndef main() -> Result[(), AllocError | IoError | FormatError | math.MathError]:\n{body}    Ok(())\n"
        ),
        optimize,
    )
}

fn run_program(source_text: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, source_text).unwrap();
    plenty::check_file(&source, None).unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable_with_options(
        &source,
        &program,
        None,
        &plenty::CompileOptions {
            optimize,
            ..Default::default()
        },
    )
    .unwrap();
    let output = Command::new(program).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run(body: &str) -> String {
    run_mode(body, false)
}

fn check_error(source_text: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, source_text).unwrap();
    plenty::check_file(&source, None).unwrap_err().to_string()
}

const ARITHMETIC: &str = r#"    a = Complex(1.5, 2.0)
    b = Complex(-0.5, 4.0)
    print(f"{a + b} {a - b} {a * b} {a / b} {-a}"?)?
    print(f"{a.conj()} {a.scale(2.0)} {a.abs()} {Complex(3.0, 4.0).abs()} {Complex(3.0, 4.0).norm()}"?)?
    print(a == Complex(1.5, 2.0), a != b, a.re, a.im)?
    print(f"{(a * b) / b} {Complex(0.0, 1.0) * Complex(0.0, 1.0)}"?)?
    mut total = Complex(0.0, 0.0)
    for k in range(4):
        total = total + Complex(f64(k), 1.0)
    print(f"{total}"?)?
"#;

const ARITHMETIC_OUTPUT: &str = "1.0+6.0i 2.0-2.0i -8.75+5.0i 0.4461538461538462-0.4307692307692308i -1.5-2.0i\n1.5-2.0i 3.0+4.0i 2.5 5.0 25.0\nTrue True 1.5 2.0\n1.5+2.0i -1.0+0.0i\n6.0+4.0i\n";

#[test]
fn operators_follow_complex_arithmetic() {
    assert_eq!(run(ARITHMETIC), ARITHMETIC_OUTPUT);
}

#[test]
fn optimized_operators_match_debug_results() {
    assert_eq!(run_mode(ARITHMETIC, true), ARITHMETIC_OUTPUT);
}

#[test]
fn division_by_zero_and_scaling_follow_f64() {
    let output = run(
        r#"    print(f"{Complex(1.0, 2.0) / Complex(0.0, 0.0)} {Complex(0.0, 0.0) / Complex(0.0, 0.0)}"?)?
    print(f"{Complex(1.0, -1.0) / Complex(-0.0, 0.0)}"?)?
    big = 1e300
    print(f"{Complex(big, big) / Complex(big, big)} {Complex(1.0, 1.0) / Complex(1e-310, 1e-310)}"?)?
    print(f"{Complex(1.0, 0.0) / Complex(f64.INF, 0.0)} {Complex(f64.NAN, 0.0) / Complex(1.0, 0.0)}"?)?
    print(f"{Complex(big, big).abs()} {Complex(1e-300, 1e-300).abs()} {Complex(1e200, 1e200).norm()}"?)?
    print(f"{Complex(f64.INF, f64.NAN).abs()} {Complex(f64.NAN, f64.INF).abs()} {Complex(f64.NAN, 1.0).abs()} {Complex(-0.0, 0.0).abs()}"?)?
    print(f"{Complex(f64.INF, 1.0).scale(2.0)} {Complex(f64.INF, 1.0) * Complex(2.0, 0.0)}"?)?
"#,
    );
    assert_eq!(
        output,
        "inf+infi NaN+NaNi\n-inf+infi\n1.0+0.0i inf+0.0i\n0.0+0.0i NaN+NaNi\n1.4142135623730952e300 1.4142135623730952e-300 inf\ninf inf NaN 0.0\ninf+2.0i inf+NaNi\n"
    );
}

#[test]
fn equality_is_structural_f64_equality() {
    let output = run(r#"    print(Complex(0.0, 0.0) == Complex(-0.0, -0.0))?
    print(Complex(f64.NAN, 0.0) == Complex(f64.NAN, 0.0), Complex(f64.NAN, 0.0) != Complex(f64.NAN, 0.0))?
    print(Complex(1.0, 2.0) == Complex(1.0, -2.0), Complex(f64.INF, 0.0) == Complex(f64.INF, 0.0))?
"#);
    assert_eq!(output, "True\nFalse True\nFalse True\n");
}

#[test]
fn principal_branches_and_signed_zero() {
    let output = run(
        r#"    print(f"{Complex(-1.0, 0.0).arg()} {Complex(-1.0, -0.0).arg()} {Complex(0.0, 0.0).arg()}"?)?
    print(f"{Complex(-4.0, 0.0).sqrt()} {Complex(-4.0, -0.0).sqrt()} {Complex(0.0, -0.0).sqrt()} {Complex(-0.0, 0.0).sqrt()}"?)?
    print(f"{Complex(3.0, 4.0).sqrt()} {Complex(-3.0, 4.0).sqrt()} {Complex(-3.0, -4.0).sqrt()}"?)?
    print(f"{Complex(5e-324, 0.0).sqrt()} {Complex(1.7976931348623157e308, 1.7976931348623157e308).sqrt()}"?)?
    print(f"{Complex(1.0, f64.INF).sqrt()} {Complex(-f64.INF, 1.0).sqrt()} {Complex(f64.INF, 1.0).sqrt()}"?)?
    print(f"{Complex(0.0, 0.0).log()} {Complex(-0.0, -0.0).log()} {Complex(-1.0, 0.0).log()}"?)?
    print(f"{Complex(f64.INF, 0.0).exp()} {Complex(-f64.INF, 0.0).exp()} {Complex(1.0, 0.0).exp()}"?)?
    print(f"{Complex(0.0, 0.0).pow(Complex(0.0, 0.0))} {Complex(0.0, 0.0).pow(Complex(2.0, 0.0))} {Complex(f64.NAN, 0.0).pow(Complex(0.0, 0.0))}"?)?
    print(f"{Complex(0.0, 1.0).pow(Complex(0.0, 1.0))}"?)?
"#,
    );
    assert_eq!(
        output,
        "3.141592653589793 -3.141592653589793 0.0\n\
         0.0+2.0i 0.0-2.0i 0.0-0.0i 0.0+0.0i\n\
         2.0+1.0i 1.0+2.0i 1.0-2.0i\n\
         2.2227587494850775e-162+0.0i 1.4730945569055655e154+6.101757441282702e153i\n\
         inf+infi 0.0+infi inf+0.0i\n\
         -inf+0.0i -inf-3.141592653589793i 0.0+3.141592653589793i\n\
         inf+0.0i 0.0+0.0i 2.718281828459045+0.0i\n\
         1.0+0.0i 0.0+0.0i 1.0+0.0i\n\
         0.20787957635076193+0.0i\n"
    );
}

#[test]
fn polar_and_transcendental_round_trips_within_tolerance() {
    let output = run(r#"    radii = [1e-200, 0.5, 1.0, 3.0, 1e200]?
    mut checked = 0
    mut failures = 0
    for radius in &radii:
        for step in range(-6, 7):
            theta = f64(step) * 0.5
            z = from_polar(*radius, theta)
            r, angle = z.to_polar()
            if not math.isclose(r, *radius, 1e-14, 0.0)? or not math.isclose(angle, theta, 1e-14, 1e-15)?:
                failures += 1
            back = z.log().exp()
            if not math.isclose(back.re, z.re, 1e-12, 1e-12 * *radius)? or not math.isclose(back.im, z.im, 1e-12, 1e-12 * *radius)?:
                failures += 1
            root = z.sqrt()
            square = root * root
            if not math.isclose(square.re, z.re, 1e-14, 1e-14 * *radius)? or not math.isclose(square.im, z.im, 1e-14, 1e-14 * *radius)?:
                failures += 1
            checked += 1
    print(checked, failures)?
    w = Complex(1.0, 1.0).pow(Complex(2.0, 0.0))
    print(math.isclose(w.re, 0.0, 0.0, 1e-15)?, math.isclose(w.im, 2.0, 1e-15, 0.0)?)?
    print(f"{from_polar(2.0, 0.0)} {from_polar(0.0, 1.0)}"?)?
"#);
    assert_eq!(output, "65 0\nTrue True\n2.0+0.0i 0.0+0.0i\n");
}

#[test]
fn display_spells_both_parts_with_their_signs() {
    let output = run(
        r#"    print(f"{Complex(1.5, 2.0)} {Complex(1.0, -0.5)} {Complex(-1.0, -0.0)} {Complex(0.0, 0.0)}"?)?
    print(f"{Complex(f64.NAN, f64.INF)} {Complex(-f64.INF, -f64.INF)} {Complex(1e20, 1e-7)}"?)?
    print(f"[{Complex(1.25, -0.5):>12}] {Complex(1.25, -0.5)!r}"?)?
    print(std.fmt.to_string(&Complex(2.0, 3.0), 64, 4)?)?
    print(Complex(1.5, 2.0))?
"#,
    );
    assert_eq!(
        output,
        "1.5+2.0i 1.0-0.5i -1.0-0.0i 0.0+0.0i\n\
         NaN+infi -inf-infi 1e20+1e-7i\n\
         [   1.25-0.5i] std.complex.Complex(re=1.25, im=-0.5)\n\
         2.0+3.0i\n\
         std.complex.Complex(re=1.5, im=2.0)\n"
    );
}

#[test]
fn operands_must_both_be_complex() {
    let message = check_error(
        "from std.complex import Complex\ndef main() -> ():\n    z = Complex(1.0, 2.0) * 2.0\n",
    );
    assert!(
        message.contains("expected &std.complex.Complex, got &f64"),
        "{message}"
    );
}

#[test]
fn small_integral_powers_are_exact() {
    let output = run(r#"    a = Complex(1.0, 1.0)
    i = Complex(0.0, 1.0)
    print(f"{a.pow(Complex(2.0, 0.0))} {a.pow(Complex(-2.0, 0.0))} {i.pow(Complex(4.0, 0.0))} {a.powi(3)} {a.powi(0)}"?)?
    print(a.pow(Complex(2.0, 0.0)) == Complex(0.0, 2.0), i.pow(Complex(4.0, 0.0)) == Complex(1.0, 0.0))?
    exact = Complex(-1125899906842624.0, -1125899906842624.0)
    print(a.pow(Complex(100.0, 0.0)) == Complex(-1125899906842624.0, 0.0), a.powi(101) == exact)?
    general = a.pow(Complex(101.0, 0.0))
    print(general == exact, math.isclose(general.re, exact.re, 1e-13, 0.0)?, math.isclose(general.im, exact.im, 1e-13, 0.0)?)?
    print(f"{Complex(0.0, 0.0).pow(Complex(-2.0, 0.0))} {Complex(0.0, 0.0).pow(Complex(2.5, 0.0))} {Complex(0.0, 0.0).pow(Complex(1.0, 1.0))}"?)?
    print(f"{i.pow(i)} {Complex(2.0, 0.0).pow(Complex(0.5, 0.0))} {a.pow(Complex(2.0, 1e-300))}"?)?
    print(f"{Complex(2.0, 0.0).powi(-9223372036854775807 - 1)} {Complex(0.5, 0.0).powi(-3)}"?)?
"#);
    assert_eq!(
        output,
        "0.0+2.0i 0.0-0.5i 1.0+0.0i -2.0+2.0i 1.0+0.0i\n\
         True True\n\
         True True\n\
         False True True\n\
         inf+NaNi 0.0+0.0i NaN+NaNi\n\
         0.20787957635076193+0.0i 1.414213562373095+0.0i 1.2246467991473532e-16+2.0i\n\
         NaN+NaNi 8.0+0.0i\n"
    );
}

#[test]
fn complex_values_copy() {
    let output = run_program(
        r#"from std.complex import Complex

def conj_by_value(z: Complex) -> Complex:
    mut w = z
    w.im = -w.im
    w

def main() -> Result[(), AllocError | IoError]:
    a = Complex(1.0, 2.0)
    mut b = a
    b.re = 5.0
    c = conj_by_value(a)
    mut zs = [a, b]?
    zs.append(a)?
    zs[0].im = 9.0
    print(a, b, c)?
    print(zs[0].im, zs[2] == a, a + a == Complex(2.0, 4.0))?
    Ok(())
"#,
        false,
    );
    assert_eq!(
        output,
        "std.complex.Complex(re=1.0, im=2.0) std.complex.Complex(re=5.0, im=2.0) std.complex.Complex(re=1.0, im=-2.0)\n\
         9.0 True True\n"
    );
}

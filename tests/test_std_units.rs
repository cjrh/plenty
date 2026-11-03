//! `std.units` is embedded Plenty; these programs check its unit values
//! against their exact definitions, the dimension-aware functions, and the
//! temperature and `Duration` boundaries.
use std::process::Command;

const PRELUDE: &str = "import std.units as u\nfrom std.units import Length, Time, Velocity, Energy, m, s, kg, km, hour\nimport std.math as math\nimport std.time as time\nfrom std.fmt import FormatError\n";

fn run(body: &str) -> String {
    let source_text = format!(
        "{PRELUDE}def main() -> Result[(), AllocError | IoError | FormatError | math.MathError | RangeError]:\n{body}    Ok(())\n"
    );
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, source_text).unwrap();
    plenty::check_file(&source, None).unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &program, None).unwrap();
    let output = Command::new(program).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn check_error(body: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(
        &source,
        format!("{PRELUDE}def main() -> Result[(), math.MathError]:\n{body}    Ok(())\n"),
    )
    .unwrap();
    plenty::check_file(&source, None).unwrap_err().to_string()
}

#[test]
fn units_store_their_coherent_values() {
    // Exact definitions: each ratio below is an integer or a short decimal
    // in the defining document, so the f64 result must print exactly.
    let output = run(
        r#"    print(u.km.get(), u.mm.get(), u.hour.get(), u.day.get(), u.ms.get(), u.ug.get())?
    print(u.inch.get(), u.foot.get(), u.yard.get(), u.mile.get(), u.nautical_mile.get())?
    print(u.pound.get(), u.ounce.get(), u.pound_force.get(), u.gallon.get(), u.light_year.get())?
    print(u.kWh.get(), u.Wh.get(), u.bar.get(), u.atm.get(), u.torr.get() * 760.0, u.psi.get())?
    print(u.litre.get(), u.hectare.get(), u.acre.get(), u.tonne.get(), u.au.get())?
    print(u.calorie.get(), u.btu.get(), u.mmHg.get(), u.rankine.get() * 9.0, u.knot.to(u.nautical_mile / u.hour))?
    # Conversion divides by the target's coherent value, so a ratio of two
    # inexact decimals can round.
    print(u.mile.to(u.foot), u.foot.to(u.inch), u.litre.to(u.millilitre))?
"#,
    );
    assert_eq!(
        output,
        "1000.0 0.001 3600.0 86400.0 0.001 1e-9\n\
         0.0254 0.3048 0.9144 1609.344 1852.0\n\
         0.45359237 0.028349523125 4.4482216152605 0.003785411784 9460730472580800.0\n\
         3600000.0 3600.0 100000.0 101325.0 101325.0 6894.757293168361\n\
         0.001 10000.0 4046.8564224 1000.0 149597870700.0\n\
         4.184 1055.05585262 133.322387415 5.0 1.0\n\
         5280.0 12.000000000000002 1000.0000000000001\n"
    );
}

#[test]
fn derived_units_and_constants_have_their_dimensions() {
    let output = run(r#"    speed: Quantity[f64, Velocity] = 90.0 * km / hour
    energy: Quantity[f64, Energy] = u.N * m
    print(speed, energy, u.c, u.e, u.k_B.get(), u.N_A.get())?
    print(u.h.get(), u.R.get(), u.g_n, u.eV.to(u.J), u.knot.to(km / hour))?
    print(u.V == u.W / u.A, u.ohm == u.V / u.A, u.T == u.Wb / (m * m), u.Sv == u.Gy)?
    print(f"{u.hbar.get():.10e} {u.G.get():.5e} {u.horsepower.get():.8f} {u.psi.get():.6f}"?)?
    print(u.degree.to(u.arcminute), f"{180.0 * u.degree}"?, u.rad, u.degree.get() == f64.PI / 180.0)?
"#);
    assert_eq!(
        output,
        "25.0 m/s 1.0 m^2*kg/s^2 299792458.0 m/s 1.602176634e-19 A*s 1.380649e-23 6.02214076e23\n\
         6.62607015e-34 8.31446261815324 9.80665 m/s^2 1.602176634e-19 1.852\n\
         True True True True\n\
         1.0545718176e-34 6.67430e-11 745.69987158 6894.757293\n\
         60.0 3.141592653589793 1.0 True\n"
    );
}

#[test]
fn functions_keep_and_derive_dimensions() {
    let output = run(
        r#"    print(u.sqrt(16.0 * m * m)?, u.cbrt(8.0 * u.litre)?.to(u.cm), u.hypot(3.0 * m, 4.0 * m))?
    print(f"{u.hypot(3e200 * m, 4e200 * m).get():.12e}"?, u.hypot(0.0 * m, -0.0 * m), u.hypot(f64.INF * m, f64.NAN * m))?
    xs = [1.0 * m, 1e16 * m, 1.0 * m, -1e16 * m]?
    empty: list[Quantity[f64, Length]] = []?
    print(u.sum(&xs), u.mean(&xs).unwrap(), u.mean(&empty).is_none())?
    print(math.min(2.0 * s, 1.0 * s), math.max(2.0 * s, 1.0 * s), u.clamp(5.0 * m, 0.0 * m, 3.0 * m), u.clamp(-1.0 * m, 0.0 * m, 3.0 * m))?
    print(u.abs(-3.0 * u.kW).to(u.W), u.abs(Quantity[i64, Length](-3)), u.clamp(Quantity[i64, Length](3), Quantity[i64, Length](0), Quantity[i64, Length](2)))?
    print(f"{u.sin(30.0 * u.degree)?.get():.12f} {u.cos(u.rad * 0.0)?} {u.atan2(1.0 * m, 1.0 * m)?.to(u.degree)}"?)?
    print(u.exp(0.0 * u.rad)?, u.log(1.0 * u.rad)?, u.log10(100.0 * u.rad)?, u.log2(8.0 * u.rad)?, u.asin(1.0 * u.rad)?.to(u.degree))?
    print(u.sqrt(-1.0 * m * m).is_err(), u.log(0.0 * u.rad).is_err())?
"#,
    );
    assert_eq!(
        output,
        "4.0 m 20.0 5.0 m\n\
         5.000000000000e+200 0.0 m inf m\n\
         2.0 m 0.5 m True\n\
         1.0 s 2.0 s 3.0 m 0.0 m\n\
         3000.0 3 m 2 m\n\
         0.500000000000 1.0 45.0\n\
         1.0 0.0 2.0 3.0 90.0\n\
         True True\n"
    );
}

#[test]
fn temperatures_and_durations_convert_at_the_boundary() {
    let output = run(
        r#"    print(u.from_celsius(25.0), u.to_celsius(u.from_celsius(-40.0)), u.to_celsius(u.from_fahrenheit(212.0)))?
    print(u.to_fahrenheit(u.from_celsius(100.0)), u.to_fahrenheit(u.from_celsius(-40.0)), u.from_fahrenheit(98.6))?
    print(u.to_duration(1.5 * s)?.as_nanoseconds(), u.to_duration(2.5 * u.ns)?.as_nanoseconds(), u.to_duration(0.0 * s)?.is_zero())?
    print(u.from_duration(&time.milliseconds(250)), u.from_duration(&time.nanoseconds(18446744073709551615)))?
    long = time.nanoseconds(10_000_000_000_000_001)
    print(u.to_duration(u.from_duration(&long))?.as_nanoseconds())?
    print(u.to_duration(-1.0 * s).is_err(), u.to_duration(f64.NAN * s).is_err(), u.to_duration(18446744074.0 * s).is_err())?
    print(u.to_duration(18446744073.7095 * s)?.as_seconds(), u.to_duration(f64.INF * s).is_err())?
"#,
    );
    assert_eq!(
        output,
        "298.15 K -40.0 100.0\n\
         212.0 -40.0 310.15 K\n\
         1500000000 2 True\n\
         0.25 s 18446744073.709553 s\n\
         10000000000000002\n\
         True True True\n\
         18446744073 True\n"
    );
}

#[test]
fn dimension_errors_name_std_units_aliases() {
    let error = check_error("    v: Quantity[f64, u.Velocity] = u.m\n");
    assert!(
        error.contains(
            "note: dimension alias `u.Velocity` = `std.units.Length * Pow[std.units.Time, -1]`"
        ),
        "{error}"
    );
    let error = check_error("    side = u.sqrt(2.0 * u.litre)?\n");
    assert!(error.contains("not a whole dimension"), "{error}");
    assert!(
        error.contains("note: dimension alias `Volume` = `Pow[std.units.Length, 3]`"),
        "{error}"
    );
    let error = check_error("    angle = u.sin(2.0 * m)?\n");
    assert!(error.contains("std.units.Length"), "{error}");
}

//! The bundled standard library is checked and emitted as ordinary Plenty.
use std::process::Command;

mod support;

fn run(body: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(
        &source,
        format!("import std.math as math\ntype Real = Result[f64, math.MathError]\ntype Integer = Result[u64, math.MathError]\ntype Rounded = Result[i64, math.MathError]\ntype Close = Result[bool, math.MathError]\ndef main() -> Result[(), AllocError | IoError | math.MathError]:\n{body}    Ok(())\n"),
    )
    .unwrap();
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

#[test]
fn floating_algorithms_and_generic_helpers() {
    assert_eq!(run(r#"    print(math.sqrt(9.0)?)?
    print(math.log2(8.0)?)?
    print(math.log10(100.0)?)?
    print(math.log_base(8.0, 2.0)?)?
    print(math.exp2(3.0)?)?
    print(math.expm1(1e-20)? == 1e-20)?
    print(math.log1p(1e-20)? == 1e-20)?
    print(math.isclose(math.cbrt(-8.0)?, -2.0, 1e-14, 0.0)?)?
    print(math.sin(0.0)?)?
    print(math.cos(0.0)?)?
    print(math.tan(0.0)?)?
    print(math.asin(0.0)?)?
    print(math.acos(1.0)?)?
    print(math.atan(0.0)?)?
    print(math.atan2(0.0, 1.0)?)?
    print(math.sinh(0.0)?)?
    print(math.cosh(0.0)?)?
    print(math.tanh(0.0)?)?
    print(math.asinh(0.0)?)?
    print(math.acosh(1.0)?)?
    print(math.atanh(0.0)?)?
    print(math.isclose(math.radians(180.0), f64.PI, 1e-15, 0.0)?)?
    print(math.degrees(f64.PI))?
    print(math.pow(2.0, -3.0)?)?
    print(math.fmod(-7.5, 2.0)?)?
    print(math.floor(-2.5)?)?
    print(math.ceil(-2.5)?)?
    print(math.trunc(-2.5)?)?
    print(math.abs(-7i32))?
    print(math.min(3u8, 2u8))?
    print(math.max(3.0f32, 2.0f32))?
    items = [2i16, 3, 4]?
    print(math.sum(&items, 1i16))?
    print(math.prod(&items, 1i16))?
"#), "3.0\n3.0\n2.0\n3.0\n8.0\nTrue\nTrue\nTrue\n0.0\n1.0\n0.0\n0.0\n0.0\n0.0\n0.0\n0.0\n1.0\n0.0\n0.0\n0.0\n0.0\nTrue\n180.0\n0.125\n-1.5\n-3\n-2\n-2\n7\n2\n3.0\n10\n24\n");
}

#[test]
fn round_uses_ties_to_even_and_checked_integer_bounds() {
    let output = run(r#"    print(math.round(2.5)?)?
    print(math.round(3.5)?)?
    print(math.round(-2.5)?)?
    print(math.round(-3.5)?)?
    print(math.round(-0.0)?)?
    print(math.round(-9223372036854775808.0)?)?
    print(math.round(9223372036854775808.0) == Rounded.Err(math.MathError.Overflow))?
    print(math.round(f64.INF) == Rounded.Err(math.MathError.Overflow))?
    print(math.round(f64.NAN) == Rounded.Err(math.MathError.Domain))?
    print((2.5f32).round())?
    print((-3.5f32).round())?
    print((0.0).copysign((-0.1).round()))?
"#);
    assert_eq!(
        output,
        "2\n4\n-2\n-4\n0\n-9223372036854775808\nTrue\nTrue\nTrue\n2.0\n-4.0\n-0.0\n"
    );
}

#[test]
fn quotient_and_remainder_agree_at_integer_edges() {
    let output = run(r#"    print(math.divmod(7, 3)?)?
    print(math.divmod(-7, 3)?)?
    print(math.divmod(7, -3)?)?
    print(math.divmod(-7, -3)?)?
    print(math.divmod(i64.MIN, 1)?)?
    print(math.divmod(u64.MAX, 2u64)?)?
    print(math.divmod(-128i8, 3i8)?)?
    print(math.divmod(255u8, 255u8)?)?
    print(math.divmod(1, 0) == Result[(i64, i64), math.MathError].Err(math.MathError.Domain))?
    print(math.divmod(i64.MIN, -1) == Result[(i64, i64), math.MathError].Err(math.MathError.Overflow))?
    print(math.divmod(i8.MIN, -1i8) == Result[(i8, i8), math.MathError].Err(math.MathError.Overflow))?
"#);
    assert_eq!(output, "(2, 1)\n(-3, 2)\n(-3, -2)\n(2, -1)\n(-9223372036854775808, 0)\n(9223372036854775807, 1)\n(-43, 1)\n(1, 0)\nTrue\nTrue\nTrue\n");
}

#[test]
fn floating_divmod_preserves_signs_and_recovers_errors() {
    let output = run(r#"    print(math.divmod_float(7.5, 2.0)?)?
    print(math.divmod_float(-7.5, 2.0)?)?
    print(math.divmod_float(7.5, -2.0)?)?
    print(math.divmod_float(-7.5, -2.0)?)?
    print(math.divmod_float(1.0, 0.1)?)?
    q, r = math.divmod_float(-0.0, 2.0)?
    print(q.copysign(q))?
    print(r)?
    q2, r2 = math.divmod_float(0.0, -2.0)?
    print(q2)?
    print(r2)?
    print(math.divmod_float(1.0, 0.0) == Result[(f64, f64), math.MathError].Err(math.MathError.Domain))?
    print(math.divmod_float(f64.INF, 2.0) == Result[(f64, f64), math.MathError].Err(math.MathError.Domain))?
    print(math.divmod_float(f64.NAN, 2.0) == Result[(f64, f64), math.MathError].Err(math.MathError.Domain))?
    print(math.divmod_float(1e308, 1e-308) == Result[(f64, f64), math.MathError].Err(math.MathError.Overflow))?
"#);
    assert_eq!(output, "(3.0, 1.5)\n(-4.0, 0.5)\n(-4.0, -0.5)\n(3.0, -1.5)\n(9.0, 0.09999999999999995)\n-0.0\n0.0\n-0.0\n-0.0\nTrue\nTrue\nTrue\nTrue\n");
}

#[test]
fn collection_extrema_and_borrowed_iterable_folds() {
    let output = run(r#"    values = [4i16, -2, 7]?
    print(math.min_all(&values).unwrap())?
    print(math.max_all(&values).unwrap())?
    empty: list[i64] = []?
    print(math.min_all(&empty) == Option[i64].Nothing)?
    print(math.max_all(&empty) == Option[i64].Nothing)?
    words = ['z', 'a', 'b']?
    print(math.min_all(&words).unwrap())?
    print(math.max_all(&words).unwrap())?
    zeros = [-0.0, 0.0]?
    print(math.min_all(&zeros).unwrap())?
    print(math.max_all(&zeros).unwrap())?
    interval = range(1, 5)
    print(math.sum_iter(&interval, 0))?
    print(math.prod_iter(&interval, 1))?
    unique = {2, 3, 4}?
    print(math.sum_iter(&unique, 0))?
    print(math.prod_iter(&unique, 1))?
    mapping = {2: 'a', 3: 'b'}?
    print(math.sum_iter(&mapping, 0))?
    print(math.prod_iter(&mapping, 1))?
    print(math.sum_iter(&values, 0i16))?
    print(math.prod_iter(&empty, 7))?
    print(len(values))?
    print(len(unique))?
    print(len(mapping))?
"#);
    assert_eq!(
        output,
        "-2\n7\nTrue\nTrue\na\nz\n-0.0\n-0.0\n10\n24\n9\n24\n5\n6\n9\n7\n3\n3\n2\n"
    );
}

#[test]
fn standard_math_constants_are_values_with_aliases() {
    let output = run(r#"    print(math.pi == f64.PI)?
    print(math.e == f64.E)?
    print(math.tau == f64.TAU)?
    print(math.isinf(math.inf))?
    print(math.isnan(math.nan))?
"#);
    assert_eq!(output, "True\nTrue\nTrue\nTrue\nTrue\n");
}

#[test]
fn recoverable_errors_and_ieee_special_values() {
    assert_eq!(
        run(
            r#"    print(math.sqrt(-1.0) == Real.Err(math.MathError.Domain))?
    print(math.log(0.0) == Real.Err(math.MathError.Domain))?
    print(math.log1p(-1.0) == Real.Err(math.MathError.Domain))?
    print(math.log_base(2.0, 1.0) == Real.Err(math.MathError.Domain))?
    print(math.asin(2.0) == Real.Err(math.MathError.Domain))?
    print(math.acosh(0.0) == Real.Err(math.MathError.Domain))?
    print(math.atanh(1.0) == Real.Err(math.MathError.Domain))?
    print(math.sin(f64.INF) == Real.Err(math.MathError.Domain))?
    print(math.exp(1000.0) == Real.Err(math.MathError.Overflow))?
    print(math.cosh(1000.0) == Real.Err(math.MathError.Overflow))?
    print(math.pow(-2.0, 0.5) == Real.Err(math.MathError.Domain))?
    print(math.pow(0.0, -1.0) == Real.Err(math.MathError.Domain))?
    print(math.pow(2.0, 1024.0) == Real.Err(math.MathError.Overflow))?
    print(math.fmod(1.0, 0.0) == Real.Err(math.MathError.Domain))?
    print(math.floor(f64.NAN) == Rounded.Err(math.MathError.Domain))?
    print(math.ceil(f64.INF) == Rounded.Err(math.MathError.Overflow))?
    print(math.trunc(9223372036854775808.0) == Rounded.Err(math.MathError.Overflow))?
    print(math.trunc(-9223372036854775808.0)? == i64.MIN)?
    print(math.isnan(math.sqrt(f64.NAN)?))?
    print(math.sqrt(f64.INF)?)?
    print(math.exp(-1000.0)?)?
    print(math.isfinite(f64.NAN))?
    print(math.isfinite(f64.INF))?
    print(math.isfinite(0.0))?
    print(math.isinf(-f64.INF))?
    print(math.copysign(1.0, -0.0))?
    print(math.fabs(-0.0))?
    print(math.copysign(1.0, math.sqrt(-0.0)?))?
    print(math.isclose(f64.INF, f64.INF, 1e-9, 0.0)?)?
    print(math.isclose(f64.INF, -f64.INF, f64.INF, f64.INF)?)?
    print(math.isclose(f64.NAN, f64.NAN, 1e-9, 0.0)?)?
    print(math.isclose(1.0, 1.0, -1.0, 0.0) == Close.Err(math.MathError.Domain))?
"#
        ),
        format!(
            "{}inf\n0.0\nFalse\nFalse\nTrue\nTrue\n-1.0\n0.0\n-1.0\nTrue\nFalse\nFalse\nTrue\n",
            "True\n".repeat(19)
        )
    );
}

#[test]
fn nontrivial_float_values_match_reference_and_integer_results_match_wider_oracles() {
    let mut body = String::new();
    let cases = [
        ("exp(0.75)", 0.75f64.exp()),
        ("sin(0.75)", 0.75f64.sin()),
        ("cos(0.75)", 0.75f64.cos()),
        ("tan(0.75)", 0.75f64.tan()),
        ("asin(0.75)", 0.75f64.asin()),
        ("acos(0.75)", 0.75f64.acos()),
        ("atan(0.75)", 0.75f64.atan()),
        ("atan2(0.75, -0.5)", 0.75f64.atan2(-0.5)),
        ("sinh(0.75)", 0.75f64.sinh()),
        ("cosh(0.75)", 0.75f64.cosh()),
        ("tanh(0.75)", 0.75f64.tanh()),
        ("asinh(0.75)", 0.75f64.asinh()),
        ("acosh(1.75)", 1.75f64.acosh()),
        ("atanh(0.75)", 0.75f64.atanh()),
    ];
    for (expr, expected) in cases {
        body.push_str(&format!(
            "    print(math.isclose(math.{expr}?, {expected:?}, 1e-14, 0.0)?)?\n"
        ));
    }
    let mut assertions = cases.len();
    // Pascal's recurrence is independent of the library's cancellation algorithm.
    let mut previous = vec![1u128];
    for n in 1..=70 {
        let mut row = vec![1u128; n + 1];
        for k in 1..n {
            row[k] = previous[k - 1] + previous[k];
        }
        for k in [0, 1, n / 2, n - 1, n] {
            let expected = match u64::try_from(row[k]) {
                Ok(value) => format!("Integer.Ok({value}u64)"),
                Err(_) => "Integer.Err(math.MathError.Overflow)".into(),
            };
            body.push_str(&format!(
                "    print(math.comb({n}u64, {k}u64) == {expected})?\n"
            ));
            assertions += 1;
        }
        previous = row;
    }
    // Test either side of large perfect squares without floating conversion.
    for root in [2u64, 3, 100_000, 4_294_967_294, 4_294_967_295] {
        let square = root * root;
        for (n, expected) in [(square - 1, root - 1), (square, root), (square + 1, root)] {
            body.push_str(&format!(
                "    print(math.isqrt({n}u64) == {expected}u64)?\n"
            ));
            assertions += 1;
        }
    }
    assert_eq!(run(&body), "True\n".repeat(assertions));
}

#[test]
fn generic_helpers_preserve_magnitudes_ties_nan_and_empty_start_values() {
    assert_eq!(
        run(r#"    print(math.abs(5u64))?
    print(math.copysign(1.0, math.abs(-0.0)))?
    print(math.min(-0.0, 0.0))?
    print(math.isnan(math.min(f64.NAN, 1.0)))?
    print(math.max(1.0, f64.NAN))?
    empty: list[f64] = []?
    print(math.sum(&empty, 2.5))?
    print(math.prod(&empty, 2.5))?
"#),
        "5\n1.0\n-0.0\nTrue\n1.0\n2.5\n2.5\n"
    );
}

#[test]
fn tolerance_comparison_covers_relative_absolute_and_special_input_rules() {
    assert_eq!(
        run(r#"    print(math.isclose(1.0, 1.0000000001, 1e-9, 0.0)?)?
    print(math.isclose(1.0, 1.0001, 1e-9, 0.0)?)?
    print(math.isclose(0.0, 1e-12, 0.0, 1e-10)?)?
    print(math.isclose(0.0, 1e-12, 1e-9, 0.0)?)?
    print(math.isclose(1.0, f64.INF, f64.INF, f64.INF)?)?
    print(math.isclose(1.0, 1.0, f64.NAN, f64.NAN)?)?
    print(math.isclose(1.0, 1.0, 0.0, -1.0) == Close.Err(math.MathError.Domain))?
    print(math.pow(f64.NAN, 0.0)?)?
    print(math.pow(1.0, f64.NAN)?)?
    print(math.pow(0.0, -f64.INF)?)?
"#),
        "True\nFalse\nTrue\nFalse\nFalse\nTrue\nTrue\n1.0\n1.0\ninf\n"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn math_algorithms_and_error_construction_allocate_nothing() {
    let output = run(r#"    values = [2u64, 3, 4]?
    print('__test_begin_no_allocations__')?
    print('__test_fail_allocations_after_0__')?
    root = math.sqrt(4.0)?
    domain = math.sqrt(-1.0)
    overflow = math.exp(1000.0)
    near = math.isclose(1.0, 1.0000000001, 1e-9, 0.0)?
    choose = math.comb(67, 33)?
    integer = math.isqrt(u64.MAX)
    total = math.sum(&values, 0u64)
    rounded = math.round(3.5)?
    pair = math.divmod(u64.MAX, 2u64)?
    float_pair = math.divmod_float(-7.5, 2.0)?
    lowest = math.min_all(&values).unwrap()
    highest = math.max_all(&values).unwrap()
    interval = range(1, 5)
    folded = math.sum_iter(&interval, 0)
    product = math.prod_iter(&interval, 1)
    constant = math.pi
    print('__test_restore_allocations__')?
    print('__test_end_no_allocations__')?
    print(root)?
    print(domain == Real.Err(math.MathError.Domain))?
    print(overflow == Real.Err(math.MathError.Overflow))?
    print(near)?
    print(choose)?
    print(integer)?
    print(total)?
    print(rounded)?
    print(pair)?
    print(float_pair)?
    print(lowest)?
    print(highest)?
    print(folded)?
    print(product)?
    print(constant == f64.PI)?
"#);
    let visible: Vec<_> = output
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(
        visible.join("\n"),
        "2.0\nTrue\nTrue\nTrue\n14226520737620288370\n4294967295\n9\n4\n(9223372036854775807, 1)\n(-4.0, 0.5)\n2\n4\n10\n24\nTrue"
    );
}

#[test]
fn fixed_width_integer_algorithms_cover_the_range() {
    assert_eq!(run(r#"    print(math.factorial(0)?)?
    print(math.factorial(20)?)?
    print(math.factorial(21) == Integer.Err(math.MathError.Overflow))?
    print(math.factorial(-1) == Integer.Err(math.MathError.Domain))?
    print(math.gcd(0, 0))?
    print(math.gcd(u64.MAX, u64.MAX))?
    print(math.gcd(48, 18))?
    print(math.lcm(0, u64.MAX)?)?
    print(math.lcm(12, 18)?)?
    print(math.lcm(u64.MAX, 2) == Integer.Err(math.MathError.Overflow))?
    print(math.isqrt(0))?
    print(math.isqrt(1))?
    print(math.isqrt(15))?
    print(math.isqrt(16))?
    print(math.isqrt(u64.MAX))?
    print(math.perm(5, 3)?)?
    print(math.perm(5, 6)?)?
    print(math.perm(u64.MAX, 0)?)?
    print(math.perm(u64.MAX, 1)?)?
    print(math.perm(u64.MAX, 2) == Integer.Err(math.MathError.Overflow))?
    print(math.comb(5, 2)?)?
    print(math.comb(5, 6)?)?
    print(math.comb(u64.MAX, 1)?)?
    print(math.comb(67, 33)?)?
    print(math.comb(68, 34) == Integer.Err(math.MathError.Overflow))?
"#), "1\n2432902008176640000\nTrue\nTrue\n0\n18446744073709551615\n6\n0\n36\nTrue\n0\n1\n3\n4\n4294967295\n60\n0\n1\n18446744073709551615\nTrue\n10\n0\n18446744073709551615\n14226520737620288370\nTrue\n");
}

#[test]
fn every_listed_standard_module_imports() {
    // The unknown-module diagnostic and module loading share one table; this
    // keeps the listed names importable (issue #217).
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, "import std.missing\ndef main() -> ():\n    pass\n").unwrap();
    let message = plenty::check_file(&source, None).unwrap_err().to_string();
    let listed = message.split("available modules: ").nth(1).unwrap();
    let names: Vec<_> = listed.lines().next().unwrap().split(", ").collect();
    assert!(names.contains(&"std.db.sqlite"), "{message}");
    let imports: String = names
        .iter()
        .map(|name| format!("import {name}\n"))
        .collect();
    std::fs::write(&source, format!("{imports}def main() -> ():\n    pass\n")).unwrap();
    if let Err(error) = plenty::check_file(&source, None) {
        panic!("{error}");
    }
}

#[test]
fn std_is_reserved_and_unknown_modules_fail_without_filesystem_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, "import std.missing\ndef main() -> ():\n    pass\n").unwrap();
    assert!(plenty::check_file(&source, None)
        .unwrap_err()
        .to_string()
        .contains("unknown standard library module"));
    for name in ["std", "std.plenty", "std.plentyi"] {
        let path = dir.path().join(name);
        if name == "std" {
            std::fs::create_dir(&path).unwrap();
        } else {
            std::fs::write(&path, "").unwrap();
        }
        assert!(plenty::check_file(&source, None)
            .unwrap_err()
            .to_string()
            .contains("`std` is reserved"));
        if name == "std" {
            std::fs::remove_dir(path).unwrap();
        } else {
            std::fs::remove_file(path).unwrap();
        }
    }
}

#[test]
fn aliases_and_diamonds_share_std_nominal_types_and_privacy() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("helper.plenty"), "from std.math import sqrt, MathError\npub def root(x: f64) -> Result[f64, MathError]:\n    sqrt(x)\n").unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, "import helper\nimport std.math\nfrom std.math import MathError as Error\ndef main() -> ():\n    value: Result[f64, Error] = helper.root(-1.0)\n    print(value == Result[f64, Error].Err(std.math.MathError.Domain)).unwrap()\n").unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &program, None).unwrap();
    let output = Command::new(program).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"True\n");
    std::fs::write(
        &source,
        "from std.math import unary_result\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
    assert!(plenty::check_file(&source, None)
        .unwrap_err()
        .to_string()
        .contains("private"));
}

#[test]
fn relocated_compiler_imports_bundled_std_without_repository_or_rust_tools() {
    let dir = tempfile::tempdir().unwrap();
    let compiler = dir.path().join("compiler");
    support::install_executable(
        std::path::Path::new(env!("CARGO_BIN_EXE_plenty")),
        &compiler,
    );
    let source = dir.path().join("main.plenty");
    std::fs::write(
        &source,
        "from std.math import sqrt\ndef main() -> ():\n    print(sqrt(81.0).unwrap()).unwrap()\n",
    )
    .unwrap();
    let check = Command::new(&compiler)
        .arg("--check")
        .arg(&source)
        .env("PATH", "")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let compile = Command::new(compiler)
        .arg("--compile")
        .arg(source)
        .arg("-o")
        .arg(dir.path().join("program"))
        .env("PATH", "/usr/bin:/bin")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let output = Command::new(dir.path().join("program")).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"9.0\n");
}

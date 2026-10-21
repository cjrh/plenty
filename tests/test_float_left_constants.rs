//! A float constant on the left of `-` or `/`, and the inline `copysign` and
//! `fabs` (issue #380). x86-64 Cranelift builds a left float constant with
//! `movabs` + `movq` at each use; lowering reads it from the constant pool
//! instead. `copysign` and `fabs` were runtime calls and are now bit masks.
//! None of this may change a result: `EXPECTED` is the output of the compiler
//! before the change, and every literal form is printed beside the same
//! operation with the constant passed as an argument. Signs of NaN results
//! are printed through `copysign` of a non-constant operand.
mod support;

const PROGRAM: &str = r#"
def sub_l(x: f64) -> f64:
    1.5 - x

def sub_p(c: f64, x: f64) -> f64:
    c - x

def div_l(x: f64) -> f64:
    1.5 / x

def div_p(c: f64, x: f64) -> f64:
    c / x

def subz_l(x: f64) -> f64:
    -0.0 - x

def divn_l(x: f64) -> f64:
    -3.0 / x

def sub32_l(x: f32) -> f32:
    1.5 - x

def sub32_p(c: f32, x: f32) -> f32:
    c - x

def div32_l(x: f32) -> f32:
    1.5 / x

def div32_p(c: f32, x: f32) -> f32:
    c / x

def cs_p(x: f64, y: f64) -> f64:
    x.copysign(y)

def cs_l(y: f64) -> f64:
    (1.5).copysign(y)

def cs_z(y: f64) -> f64:
    (0.0).copysign(y)

def cs_r(x: f64) -> f64:
    x.copysign(-1.0)

def cs_rp(x: f64) -> f64:
    x.copysign(2.0)

def cs32_p(x: f32, y: f32) -> f32:
    x.copysign(y)

def cs32_l(y: f32) -> f32:
    (1.5f32).copysign(y)

def abs_p(x: f64) -> f64:
    x.fabs()

def abs32_p(x: f32) -> f32:
    x.fabs()

def fold_l() -> f64:
    1.5 - 0.5

def sign(x: f64) -> f64:
    (1.0).copysign(x)

def sum_l(n: i64, x0: f64) -> f64:
    mut s = 0.0
    mut x = x0
    mut i = 0
    while i < n:
        s = s + (1.5 - x) + 2.5 / x
        x = x + 0.25
        i = i + 1
    s

def sum_p(n: i64, x0: f64, a: f64, b: f64) -> f64:
    mut s = 0.0
    mut x = x0
    mut i = 0
    while i < n:
        s = s + (a - x) + b / x
        x = x + 0.25
        i = i + 1
    s

def main() -> Result[(), AllocError | IoError]:
    values = [0.0, -0.0, 1.5, -1.5, 1.0e308, -1.0e308, 5.0e-324, -5.0e-324, f64.INF, -f64.INF, f64.NAN, -f64.NAN]?
    for x in values:
        print(sub_l(x), sub_p(1.5, x), subz_l(x), sub_p(-0.0, x), sign(sub_l(x)), sign(sub_p(1.5, x)))?
        print(div_l(x), div_p(1.5, x), divn_l(x), div_p(-3.0, x), sign(div_l(x)), sign(divn_l(x)))?
        print(cs_l(x), cs_p(1.5, x), cs_z(x), cs_p(0.0, x), cs_r(x), cs_p(x, -1.0), cs_rp(x), cs_p(x, 2.0))?
        print(abs_p(x), sign(abs_p(x)), sign(cs_r(x)), sign(cs_rp(x)), sign(cs_p(x, x)))?
        print((2.5).copysign(-0.0), (-2.5).copysign(0.0), (f64.NAN).copysign(-1.0) < 0.0, sign((-f64.NAN).copysign(1.0)), (0.0).fabs(), (-0.0).fabs(), (-f64.INF).fabs())?
    halves: list[f32] = [0.0, -0.0, 1.5, -1.5, 3.0e38, -3.0e38, 1.0e-45, -1.0e-45, f32.INF, -f32.INF, f32.NAN, -f32.NAN]?
    for h in halves:
        print(sub32_l(h), sub32_p(1.5, h), div32_l(h), div32_p(1.5, h), cs32_l(h), cs32_p(1.5, h), abs32_p(h))?
    print(sum_l(1000, 0.75), sum_p(1000, 0.75, 1.5, 2.5), fold_l())?
    Ok(())
"#;

/// Output of the compiler before issue #380, identical in debug and release.
const EXPECTED: &str = "\
1.5 1.5 -0.0 -0.0 1.0 1.0
inf inf -inf -inf 1.0 -1.0
1.5 1.5 0.0 0.0 -0.0 -0.0 0.0 0.0
0.0 1.0 -1.0 1.0 1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
1.5 1.5 0.0 0.0 1.0 1.0
-inf -inf inf inf -1.0 1.0
-1.5 -1.5 -0.0 -0.0 -0.0 -0.0 0.0 0.0
0.0 1.0 -1.0 1.0 -1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
0.0 0.0 -1.5 -1.5 1.0 1.0
1.0 1.0 -2.0 -2.0 1.0 -1.0
1.5 1.5 0.0 0.0 -1.5 -1.5 1.5 1.5
1.5 1.0 -1.0 1.0 1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
3.0 3.0 1.5 1.5 1.0 1.0
-1.0 -1.0 2.0 2.0 -1.0 1.0
-1.5 -1.5 -0.0 -0.0 -1.5 -1.5 1.5 1.5
1.5 1.0 -1.0 1.0 -1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
-1e308 -1e308 -1e308 -1e308 -1.0 -1.0
1.5e-308 1.5e-308 -2.9999999999999997e-308 -2.9999999999999997e-308 1.0 -1.0
1.5 1.5 0.0 0.0 -1e308 -1e308 1e308 1e308
1e308 1.0 -1.0 1.0 1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
1e308 1e308 1e308 1e308 1.0 1.0
-1.5e-308 -1.5e-308 2.9999999999999997e-308 2.9999999999999997e-308 -1.0 1.0
-1.5 -1.5 -0.0 -0.0 -1e308 -1e308 1e308 1e308
1e308 1.0 -1.0 1.0 -1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
1.5 1.5 -5e-324 -5e-324 1.0 1.0
inf inf -inf -inf 1.0 -1.0
1.5 1.5 0.0 0.0 -5e-324 -5e-324 5e-324 5e-324
5e-324 1.0 -1.0 1.0 1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
1.5 1.5 5e-324 5e-324 1.0 1.0
-inf -inf inf inf -1.0 1.0
-1.5 -1.5 -0.0 -0.0 -5e-324 -5e-324 5e-324 5e-324
5e-324 1.0 -1.0 1.0 -1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
-inf -inf -inf -inf -1.0 -1.0
0.0 0.0 -0.0 -0.0 1.0 -1.0
1.5 1.5 0.0 0.0 -inf -inf inf inf
inf 1.0 -1.0 1.0 1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
inf inf inf inf 1.0 1.0
-0.0 -0.0 0.0 0.0 -1.0 1.0
-1.5 -1.5 -0.0 -0.0 -inf -inf inf inf
inf 1.0 -1.0 1.0 -1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
NaN NaN NaN NaN 1.0 1.0
NaN NaN NaN NaN 1.0 1.0
1.5 1.5 0.0 0.0 NaN NaN NaN NaN
NaN 1.0 -1.0 1.0 1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
NaN NaN NaN NaN -1.0 -1.0
NaN NaN NaN NaN -1.0 -1.0
-1.5 -1.5 -0.0 -0.0 NaN NaN NaN NaN
NaN 1.0 -1.0 1.0 -1.0
-2.5 2.5 False 1.0 0.0 0.0 inf
1.5 1.5 inf inf 1.5 1.5 0.0
1.5 1.5 -inf -inf -1.5 -1.5 0.0
0.0 0.0 1.0 1.0 1.5 1.5 1.5
3.0 3.0 -1.0 -1.0 -1.5 -1.5 1.5
-3e38 -3e38 5e-39 5e-39 1.5 1.5 3e38
3e38 3e38 -5e-39 -5e-39 -1.5 -1.5 3e38
1.5 1.5 inf inf 1.5 1.5 1e-45
1.5 1.5 -inf -inf -1.5 -1.5 1e-45
-inf -inf 0.0 0.0 1.5 1.5 inf
inf inf -0.0 -0.0 -1.5 -1.5 inf
NaN NaN NaN NaN 1.5 1.5 NaN
NaN NaN NaN NaN -1.5 -1.5 NaN
-124065.1253213445 -124065.1253213445 1.0
";

fn plenty(dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn left_constants_and_sign_masks_keep_results() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("floats.plenty"), PROGRAM).unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("floats.plenty");
        let out = plenty(dir.path(), &args);
        assert_eq!(out, EXPECTED, "{mode:?}");
        // Lines of 6, 7 and 8 fields lead with literal and argument pairs,
        // except the all-literal `copysign` line.
        for line in out.lines().filter(|line| !line.contains("False")) {
            let fields: Vec<&str> = line.split(' ').collect();
            let pairs = match fields.len() {
                6 => 2,
                7 => 3,
                8 => 4,
                _ => 0,
            };
            for pair in fields[..2 * pairs].chunks(2) {
                assert_eq!(pair[0], pair[1], "{mode:?} {line}");
            }
        }
    }
}

/// The text of one function's section in a dump stage file.
fn section(dir: &std::path::Path, stage: &str, name: &str) -> String {
    let text = std::fs::read_to_string(dir.join("out").join(stage)).unwrap();
    let start = text
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("no `{name}` in {stage}"));
    let rest = &text[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

#[test]
fn float_constants_are_read_from_the_constant_pool() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("floats.plenty"), PROGRAM).unwrap();
    plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt,asm",
            "floats.plenty",
            "-o",
            "out",
        ],
    );
    for name in [
        "sub_l", "div_l", "subz_l", "divn_l", "sub32_l", "div32_l", "cs_p", "cs_l", "cs_z", "cs_r",
        "cs_rp", "cs32_p", "cs32_l", "abs_p", "abs32_p", "sum_l",
    ] {
        let body = section(dir.path(), "asm.txt", name);
        assert!(
            !body.contains("movabs") && !body.contains("movq") && !body.contains("_math"),
            "{name}:\n{body}"
        );
    }
    for name in ["sub_l", "div32_l", "sum_l"] {
        let body = section(dir.path(), "clif-opt.txt", name);
        assert!(body.contains("vconst"), "{name}:\n{body}");
    }
    // Two constants still fold, and the argument form needs no constant.
    for name in ["fold_l", "sub_p"] {
        let body = section(dir.path(), "clif-opt.txt", name);
        assert!(!body.contains("vconst"), "{name}:\n{body}");
    }
}

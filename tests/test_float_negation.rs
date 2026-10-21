//! Float negation `-x` (issue #385). Cranelift's x86-64 `fneg` builds its
//! sign mask with `movabs` + `movq` at each use; lowering emits `bxor` with
//! a constant-pool mask instead. The result must not change: `EXPECTED` is
//! the output of the compiler before the change, identical in debug and
//! release. Signs of zero and NaN results are printed through `copysign`.
mod support;

const PROGRAM: &str = r#"
def neg(x: f64) -> f64:
    -x

def neg2(x: f64) -> f64:
    -(-x)

def neg_sum(x: f64, y: f64) -> f64:
    -(x + y)

def neg32(x: f32) -> f32:
    -x

def neg32_2(x: f32) -> f32:
    -(-x)

def neg_c() -> f64:
    -(1.5)

def neg_inf() -> f64:
    -f64.INF

def neg_nan() -> f64:
    -f64.NAN

def neg_z32() -> f32:
    -(0.0f32)

def sign(x: f64) -> f64:
    (1.0).copysign(x)

def sign32(x: f32) -> f32:
    (1.0f32).copysign(x)

def neg_loop(n: i64, x0: f64, a: f64) -> f64:
    mut s = 0.0
    mut x = x0
    mut i = 0
    while i < n:
        s = s + -(x * a)
        x = -x + 0.25
        i = i + 1
    s

def main() -> Result[(), AllocError | IoError]:
    values = [0.0, -0.0, 1.5, -1.5, 1.0e308, -1.0e308, 5.0e-324, -5.0e-324, f64.INF, -f64.INF, f64.NAN, -f64.NAN]?
    for x in values:
        print(neg(x), sign(neg(x)), neg2(x), sign(neg2(x)), neg_sum(x, 0.0), sign(neg_sum(x, -0.0)))?
    halves: list[f32] = [0.0, -0.0, 1.5, -1.5, 3.0e38, -3.0e38, 1.0e-45, -1.0e-45, f32.INF, -f32.INF, f32.NAN, -f32.NAN]?
    for h in halves:
        print(neg32(h), sign32(neg32(h)), neg32_2(h), sign32(neg32_2(h)))?
    print(neg_c(), neg_inf(), sign(neg_nan()), sign(-neg_nan()), neg_z32(), sign32(neg_z32()))?
    print(neg_loop(1000, 0.75, 1.5))?
    Ok(())
"#;

/// Output of the compiler before issue #385, identical in debug and release.
const EXPECTED: &str = "\
-0.0 -1.0 0.0 1.0 -0.0 -1.0
0.0 1.0 -0.0 -1.0 -0.0 1.0
-1.5 -1.0 1.5 1.0 -1.5 -1.0
1.5 1.0 -1.5 -1.0 1.5 1.0
-1e308 -1.0 1e308 1.0 -1e308 -1.0
1e308 1.0 -1e308 -1.0 1e308 1.0
-5e-324 -1.0 5e-324 1.0 -5e-324 -1.0
5e-324 1.0 -5e-324 -1.0 5e-324 1.0
-inf -1.0 inf 1.0 -inf -1.0
inf 1.0 -inf -1.0 inf 1.0
NaN -1.0 NaN 1.0 NaN -1.0
NaN 1.0 NaN -1.0 NaN 1.0
-0.0 -1.0 0.0 1.0
0.0 1.0 -0.0 -1.0
-1.5 -1.0 1.5 1.0
1.5 1.0 -1.5 -1.0
-3e38 -1.0 3e38 1.0
3e38 1.0 -3e38 -1.0
-1e-45 -1.0 1e-45 1.0
1e-45 1.0 -1e-45 -1.0
-inf -1.0 inf 1.0
inf 1.0 -inf -1.0
NaN -1.0 NaN 1.0
NaN 1.0 NaN -1.0
-1.5 -inf -1.0 1.0 -0.0 -1.0
-187.5
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
fn float_negation_keeps_results() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("neg.plenty"), PROGRAM).unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("neg.plenty");
        assert_eq!(plenty(dir.path(), &args), EXPECTED, "{mode:?}");
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
fn float_negation_reads_its_mask_from_the_constant_pool() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("neg.plenty"), PROGRAM).unwrap();
    plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt,asm",
            "neg.plenty",
            "-o",
            "out",
        ],
    );
    for name in ["neg", "neg_sum", "neg32", "neg_loop"] {
        let asm = section(dir.path(), "asm.txt", name);
        assert!(
            asm.contains("xorp") && !asm.contains("movabs") && !asm.contains("movq"),
            "{name}:\n{asm}"
        );
        let clif = section(dir.path(), "clif-opt.txt", name);
        assert!(
            clif.contains("bxor") && !clif.contains("fneg"),
            "{name}:\n{clif}"
        );
    }
    // `-(-x)` cancels, and a negated constant folds.
    for name in ["neg2", "neg32_2", "neg_c", "neg_z32"] {
        let asm = section(dir.path(), "asm.txt", name);
        assert!(!asm.contains("xorp"), "{name}:\n{asm}");
    }
}

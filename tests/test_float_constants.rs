//! Float addition and multiplication with a literal operand (issue #370).
//! A non-NaN constant on the left moves to the right, where x86-64 reads it
//! from the constant pool instead of building it in a register each loop
//! iteration, and multiplication by `2.0` becomes `x + x`. Both rewrites must
//! give the same bits as the multiplication they replace, including signed
//! zero, infinities, overflow, subnormals and NaN signs.
mod support;

const PROGRAM: &str = r#"
def dbl_l(x: f64) -> f64:
    2.0 * x

def dbl_r(x: f64) -> f64:
    x * 2.0

def mul(x: f64, y: f64) -> f64:
    x * y

def add(x: f64, y: f64) -> f64:
    x + y

def tri_l(x: f64) -> f64:
    3.0 * x

def half_l(x: f64) -> f64:
    0.5 + x

def nan_l(x: f64) -> f64:
    f64.NAN * x

def dbl32_l(x: f32) -> f32:
    2.0 * x

def dbl32_r(x: f32) -> f32:
    x * 2.0

def mul32(x: f32, y: f32) -> f32:
    x * y

def escape(cr: f64, ci: f64, limit: i64) -> i64:
    mut zr = 0.0
    mut zi = 0.0
    mut iteration = 0
    while iteration < limit:
        rr = zr * zr
        ii = zi * zi
        if rr + ii > 4.0:
            break
        zi = (2.0 * zr) * zi + ci
        zr = rr - ii + cr
        iteration = iteration + 1
    iteration

def escape_param(cr: f64, ci: f64, limit: i64, two: f64) -> i64:
    mut zr = 0.0
    mut zi = 0.0
    mut iteration = 0
    while iteration < limit:
        rr = zr * zr
        ii = zi * zi
        if rr + ii > 4.0:
            break
        zi = (two * zr) * zi + ci
        zr = rr - ii + cr
        iteration = iteration + 1
    iteration

def sign(x: f64) -> f64:
    (1.0).copysign(x)

def main() -> Result[(), AllocError | IoError]:
    values = [0.0, -0.0, 1.5, -1.5, 1.0e308, -1.0e308, 5.0e-324, -5.0e-324, f64.INF, -f64.INF, f64.NAN, -f64.NAN]?
    for x in values:
        a = dbl_l(x)
        b = dbl_r(x)
        c = mul(2.0, x)
        print(a, b, c, sign(a), sign(b), sign(c))?
        print(tri_l(x), mul(x, 3.0), sign(tri_l(x)), sign(mul(x, 3.0)))?
        print(half_l(x), add(x, 0.5), sign(half_l(x)), sign(add(x, 0.5)))?
        print(sign(nan_l(x)), sign(mul(f64.NAN, x)))?
    halves: list[f32] = [0.0, -0.0, 1.5, 3.0e38, -f32.INF, f32.NAN]?
    for h in halves:
        print(dbl32_l(h), dbl32_r(h), mul32(2.0, h))?
    mut lit = 0
    mut param = 0
    for y in range(24):
        for x in range(32):
            cr = f64(x) / 12.0 - 2.0
            ci = f64(y) / 12.0 - 1.0
            lit = lit + escape(cr, ci, 64)
            param = param + escape_param(cr, ci, 64, 2.0)
    print(lit, param)?
    Ok(())
"#;

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

/// Every printed group must agree with the plain multiplication or addition
/// it replaces, which receives the constant as an argument.
#[test]
fn rewritten_operations_match_plain_arithmetic() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("floats.plenty"), PROGRAM).unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("floats.plenty");
        let out = plenty(dir.path(), &args);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 12 * 4 + 6 + 1, "{mode:?}\n{out}");
        for (index, line) in lines[..48].iter().enumerate() {
            let fields: Vec<&str> = line.split(' ').collect();
            let half = fields.len() / 2;
            let (values, signs) = fields.split_at(half);
            match index % 4 {
                // dbl_l, dbl_r, mul then their signs.
                0 => {
                    assert!(values.iter().all(|v| *v == values[0]), "{line}");
                    assert!(signs.iter().all(|v| *v == signs[0]), "{line}");
                }
                // The NaN constant keeps its place; signs of NaN results.
                3 => assert_eq!(fields[0], fields[1], "{mode:?} {line}"),
                _ => {
                    assert_eq!(values[0], values[1], "{mode:?} {line}");
                    assert_eq!(signs[0], signs[1], "{mode:?} {line}");
                }
            }
        }
        let doubled: Vec<&str> = lines[..48].iter().step_by(4).copied().collect();
        assert_eq!(
            doubled,
            [
                "0.0 0.0 0.0 1.0 1.0 1.0",
                "-0.0 -0.0 -0.0 -1.0 -1.0 -1.0",
                "3.0 3.0 3.0 1.0 1.0 1.0",
                "-3.0 -3.0 -3.0 -1.0 -1.0 -1.0",
                "inf inf inf 1.0 1.0 1.0",
                "-inf -inf -inf -1.0 -1.0 -1.0",
                "1e-323 1e-323 1e-323 1.0 1.0 1.0",
                "-1e-323 -1e-323 -1e-323 -1.0 -1.0 -1.0",
                "inf inf inf 1.0 1.0 1.0",
                "-inf -inf -inf -1.0 -1.0 -1.0",
                "NaN NaN NaN 1.0 1.0 1.0",
                "NaN NaN NaN -1.0 -1.0 -1.0",
            ],
            "{mode:?}"
        );
        assert_eq!(
            lines[48..54],
            [
                "0.0 0.0 0.0",
                "-0.0 -0.0 -0.0",
                "3.0 3.0 3.0",
                "inf inf inf",
                "-inf -inf -inf",
                "NaN NaN NaN",
            ],
            "{mode:?}"
        );
        let totals: Vec<&str> = lines[54].split(' ').collect();
        assert_eq!(totals[0], totals[1], "{mode:?}");
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
fn literal_operands_are_not_built_in_registers() {
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
    for name in ["dbl_l", "dbl_r", "dbl32_l", "dbl32_r"] {
        let body = section(dir.path(), "clif-opt.txt", name);
        assert!(
            body.contains("fadd") && !body.contains("fmul") && !body.contains("const"),
            "{name}:\n{body}"
        );
    }
    // A constant operand on the right is a memory operand on x86-64; on the
    // left it was materialized with `movabs` and `movq` each time.
    for name in ["tri_l", "half_l", "escape"] {
        let body = section(dir.path(), "asm.txt", name);
        assert!(
            !body.contains("movabs") && !body.contains("movq"),
            "{name}:\n{body}"
        );
    }
}

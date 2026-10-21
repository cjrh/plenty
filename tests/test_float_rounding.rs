//! `floor`, `ceil`, `trunc` and `round` on floats are inline SSE2 sequences
//! instead of `plenty_f64_math` calls (issue #386). Every result must equal
//! Rust's `floor`, `ceil`, `trunc` and `round_ties_even` bit for bit: the
//! program prints each input and result with its sign (through `copysign`,
//! which also shows the sign of a NaN), and the test recomputes them.
mod support;

const FUNCTIONS: &str = r#"
def fl(x: f64) -> f64:
    x.floor()

def ce(x: f64) -> f64:
    x.ceil()

def tr(x: f64) -> f64:
    x.trunc()

def ro(x: f64) -> f64:
    x.round()

def fl32(x: f32) -> f32:
    x.floor()

def ce32(x: f32) -> f32:
    x.ceil()

def tr32(x: f32) -> f32:
    x.trunc()

def ro32(x: f32) -> f32:
    x.round()

def folded() -> f64:
    (2.5).round() + (-0.5).floor()

def sign(x: f64) -> f64:
    (1.0).copysign(x)

def sign32(x: f32) -> f32:
    (1.0f32).copysign(x)

def line(tag: str, x: f64, a: f64, b: f64, c: f64, d: f64) -> Result[(), AllocError | IoError]:
    print(tag, x, a, b, c, d, sign(x), sign(a), sign(b), sign(c), sign(d))?
    Ok(())

def line32(tag: str, x: f32, a: f32, b: f32, c: f32, d: f32) -> Result[(), AllocError | IoError]:
    print(tag, x, a, b, c, d, sign32(x), sign32(a), sign32(b), sign32(c), sign32(d))?
    Ok(())

def show(x: f64) -> Result[(), AllocError | IoError]:
    line("D", x, fl(x), ce(x), tr(x), ro(x))

def show32(x: f32) -> Result[(), AllocError | IoError]:
    line32("F", x, fl32(x), ce32(x), tr32(x), ro32(x))

def main() -> Result[(), AllocError | IoError]:
    values = [0.0, 0.5, 1.5, 2.5, 0.49999999999999994, 1.0, 3.5, 0.7, 4503599627370495.5, 4503599627370496.0, 4503599627370497.0, 4503599627370494.5, 2251799813685247.5, 2251799813685248.5, 9007199254740991.0, 9007199254740992.0, 9007199254740994.0, 8388607.5, 8388608.5, 16777215.0, 1.7976931348623157e308, 5.0e-324, 2.2250738585072014e-308, f64.INF, f64.NAN]?
    for v in values:
        show(v)?
        show(-v)?
    halves: list[f32] = [0.0, 0.5, 1.5, 2.5, 0.49999997, 0.7, 8388607.5, 8388608.0, 8388609.0, 4194303.5, 4194304.5, 16777215.0, 16777216.0, 3.4028235e38, 1.0e-45, 1.1754944e-38, f32.INF, f32.NAN]?
    for h in halves:
        show32(h)?
        show32(-h)?
    mut k = -40
    while k <= 40:
        show(f64(k) * 0.125)?
        show32(f32(k) * 0.125f32)?
        k = k + 1
    # Random mantissas over exponents from about 2^-64 to 2^65.
    mut s = 88172645463325252u64
    mut i = 0
    while i < 4000:
        s = s ^ (s << 13)
        s = s ^ (s >> 7)
        s = s ^ (s << 17)
        mut e = i64((s >> 3) & 127u64) - 64
        mut x = f64(s >> 11) * 0.0000000000000002220446049250313
        while e > 0:
            x = x * 0.5
            e = e - 1
        while e < 0:
            x = x * 2.0
            e = e + 1
        if s & 1u64 == 1u64:
            x = -x
        show(x)?
        show32(f32(x))?
        i = i + 1
"#;

/// Literal operands, which lowering folds with Rust's functions.
const LITERALS: &[&str] = &[
    "0.0",
    "-0.0",
    "0.5",
    "-0.5",
    "1.5",
    "-1.5",
    "2.5",
    "-2.5",
    "-0.7",
    "4503599627370495.5",
    "-4503599627370495.5",
    "1.0e300",
    "-5.0e-324",
    "f64.INF",
    "-f64.INF",
    "f64.NAN",
];

const LITERALS32: &[&str] = &["-0.5f32", "2.5f32", "-0.7f32", "8388607.5f32", "f32.NAN"];

fn program() -> String {
    let mut text = FUNCTIONS.to_string();
    for literal in LITERALS {
        let x = format!("({literal})");
        text.push_str(&format!(
            "    line(\"D\", {x}, {x}.floor(), {x}.ceil(), {x}.trunc(), {x}.round())?\n"
        ));
    }
    for literal in LITERALS32 {
        let x = format!("({literal})");
        text.push_str(&format!(
            "    line32(\"F\", {x}, {x}.floor(), {x}.ceil(), {x}.trunc(), {x}.round())?\n"
        ));
    }
    text.push_str("    Ok(())\n");
    text
}

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

/// A printed value with the sign printed beside it, which tells `-NaN`
/// from `NaN`. `f32` text is the shortest that reads back as that `f32`.
fn value(text: &str, sign: &str, f32: bool) -> f64 {
    let magnitude = text.trim_start_matches('-');
    let magnitude = if f32 {
        f64::from(magnitude.parse::<f32>().unwrap())
    } else {
        magnitude.parse::<f64>().unwrap()
    };
    magnitude.copysign(sign.parse().unwrap())
}

fn check(line: &str) {
    let fields: Vec<&str> = line.split(' ').collect();
    assert_eq!(fields.len(), 11, "{line}");
    let f32 = fields[0] == "F";
    let values: Vec<f64> = (1..6)
        .map(|i| value(fields[i], fields[i + 5], f32))
        .collect();
    let x = values[0];
    let want = if f32 {
        let x = x as f32;
        [x.floor(), x.ceil(), x.trunc(), x.round_ties_even()].map(f64::from)
    } else {
        [x.floor(), x.ceil(), x.trunc(), x.round_ties_even()]
    };
    for (got, want) in values[1..].iter().zip(want) {
        let same = got.to_bits() == want.to_bits()
            || (got.is_nan() && want.is_nan() && got.is_sign_negative() == want.is_sign_negative());
        assert!(same, "{line}: want {want:?}");
    }
}

#[test]
fn rounding_matches_rust_bit_for_bit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("round.plenty"), program()).unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("round.plenty");
        let out = plenty(dir.path(), &args);
        assert_eq!(out.lines().count(), 50 + 36 + 2 * 81 + 2 * 4000 + 21);
        out.lines().for_each(check);
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
fn rounding_is_inline_and_branch_free() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("round.plenty"), program()).unwrap();
    plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt,asm",
            "round.plenty",
            "-o",
            "out",
        ],
    );
    for name in ["fl", "ce", "tr", "ro", "fl32", "ce32", "tr32", "ro32"] {
        let body = section(dir.path(), "asm.txt", name);
        assert!(
            !body.contains("call") && !body.contains("movabs"),
            "{name}:\n{body}"
        );
        // One branch only, for magnitudes too large to have a fraction.
        let jumps = body
            .lines()
            .filter(|line| {
                line.split_whitespace()
                    .nth(1)
                    .is_some_and(|i| i.starts_with('j'))
            })
            .count();
        assert_eq!(jumps, 1, "{name}:\n{body}");
    }
    for name in ["fl", "ce", "tr", "fl32", "ce32", "tr32"] {
        let body = section(dir.path(), "asm.txt", name);
        assert!(body.contains("cmplt"), "{name}:\n{body}");
    }
    let body = section(dir.path(), "clif-opt.txt", "folded");
    assert!(
        !body.contains("fsub") && !body.contains("scalar_to_vector"),
        "folded:\n{body}"
    );
}

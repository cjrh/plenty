//! Scalar float loads from list rows, views, references and fields load as
//! `f32`/`f64` rather than as an integer word and a bitcast, so x86-64 needs
//! no GPR-to-XMM move (issue #812). Folding the load into arithmetic as a
//! memory operand is issue #853. `EXPECTED` is the output of the compiler
//! before the change, identical in debug and release. Plenty cannot observe
//! NaN payloads, so the program checks NaN and zero signs.
mod support;

const PROGRAM: &str = r#"
dimension Length("m")

class Point:
    x: f64
    y: f32

@newtype
@copy
class Meters:
    value: f64

@newtype
@copy
class Grams:
    value: f32

def index64(xs: &[f64], i: i64) -> f64:
    xs[i]

def index32(xs: &[f32], i: i64) -> f32:
    xs[i]

def list_index(xs: &list[f64], i: i64) -> f64:
    xs[i]

def sum64(xs: &[f64]) -> f64:
    mut total = 0.0
    for i in range(len(xs)):
        total += xs[i]
    total

def sum32(xs: &[f32]) -> f32:
    mut total = 0.0f32
    for x in xs:
        total += *x
    total

def deref(r: &f64) -> f64:
    *r

def field64(p: &Point) -> f64:
    p.x

def field32(p: &Point) -> f32:
    p.y

def wrapped(ms: &[Meters], i: i64) -> f64:
    ms[i].value

def quantity(qs: &[Quantity[f64, Length]], i: i64) -> f64:
    qs[i].get()

def wrapped32(gs: &[Grams], i: i64) -> f32:
    gs[i].value

def quantity32(qs: &[Quantity[f32, Length]], i: i64) -> f32:
    qs[i].get()

def payload(o: &Option[f64], fallback: f64) -> f64:
    match o:
        case Some(x):
            *x
        case Nothing:
            fallback

def pair_field(t: &(i8, f64, f32)) -> f64:
    t[1]

def pair_field32(t: &(i8, f64, f32)) -> f32:
    t[2]

def sign(x: f64) -> f64:
    (1.0).copysign(x)

def sign32(x: f32) -> f32:
    (1.0f32).copysign(x)

def main() -> Result[(), AllocError | IoError]:
    xs = [0.0, -0.0, f64.NAN, -f64.NAN, -f64.INF, 5.0e-324, -1.5]?
    ys: list[f32] = [0.0, -0.0, f32.NAN, -f32.NAN, f32.INF, 1.0e-45, 2.5]?
    for i in range(len(xs)):
        a = index64(&xs, i)
        b = list_index(&xs, i)
        c = deref(&xs[i])
        print(a, sign(a), b, sign(b), c, sign(c))?
    for i in range(len(ys)):
        y = index32(&ys, i)
        print(y, sign32(y))?
    print(sum64(&xs[4:]), sum32(&ys[4:]))?
    p = Point(x=-0.0, y=-0.0f32)
    print(field64(&p), sign(field64(&p)), field32(&p), sign32(field32(&p)))?
    ms = [Meters(-0.0), Meters(2.5)]?
    qs = [Quantity[f64, Length](-0.0), Quantity[f64, Length](4.5)]?
    for i in range(2):
        print(wrapped(&ms, i), sign(wrapped(&ms, i)), quantity(&qs, i), sign(quantity(&qs, i)))?
    gs = [Grams(-0.0f32), Grams(1.5f32)]?
    rs = [Quantity[f32, Length](-0.0f32), Quantity[f32, Length](3.5f32)]?
    for i in range(2):
        print(wrapped32(&gs, i), sign32(wrapped32(&gs, i)), quantity32(&rs, i), sign32(quantity32(&rs, i)))?
    o: Option[f64] = Some(-0.0)
    print(payload(&o, 1.0), sign(payload(&o, 1.0)))?
    t = (3i8, -0.0, -f32.NAN)
    print(pair_field(&t), sign(pair_field(&t)), pair_field32(&t), sign32(pair_field32(&t)))?
    Ok(())
"#;

/// Output of the compiler before issue #812, identical in debug and release.
const EXPECTED: &str = "\
0.0 1.0 0.0 1.0 0.0 1.0
-0.0 -1.0 -0.0 -1.0 -0.0 -1.0
NaN 1.0 NaN 1.0 NaN 1.0
NaN -1.0 NaN -1.0 NaN -1.0
-inf -1.0 -inf -1.0 -inf -1.0
5e-324 1.0 5e-324 1.0 5e-324 1.0
-1.5 -1.0 -1.5 -1.0 -1.5 -1.0
0.0 1.0
-0.0 -1.0
NaN 1.0
NaN -1.0
inf 1.0
1e-45 1.0
2.5 1.0
-inf inf
-0.0 -1.0 -0.0 -1.0
-0.0 -1.0 -0.0 -1.0
2.5 1.0 4.5 1.0
-0.0 -1.0 -0.0 -1.0
1.5 1.0 3.5 1.0
-0.0 -1.0
-0.0 -1.0 NaN -1.0
";

/// Functions whose body is one or more scalar float loads.
const LOADS: [&str; 15] = [
    "index64",
    "index32",
    "list_index",
    "sum64",
    "sum32",
    "deref",
    "field64",
    "field32",
    "wrapped",
    "quantity",
    "wrapped32",
    "quantity32",
    "payload",
    "pair_field",
    "pair_field32",
];

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
fn float_loads_keep_their_bits() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("loads.plenty"), PROGRAM).unwrap();
    for mode in ["--fast-compile", "--release"] {
        assert_eq!(
            plenty(dir.path(), &[mode, "loads.plenty"]),
            EXPECTED,
            "{mode}"
        );
    }
}

#[test]
fn float_loads_are_typed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("loads.plenty"), PROGRAM).unwrap();
    plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt,asm",
            "loads.plenty",
            "-o",
            "out",
        ],
    );
    let clif = std::fs::read_to_string(dir.path().join("out/clif-opt.txt")).unwrap();
    let asm = std::fs::read_to_string(dir.path().join("out/asm.txt")).unwrap();
    for name in LOADS {
        let body = support::asm_body(&clif, name);
        assert!(
            (body.contains("load.f64") || body.contains("load.f32")) && !body.contains("bitcast"),
            "{name}:\n{body}"
        );
        if cfg!(target_arch = "x86_64") {
            let body = support::asm_body(&asm, name);
            assert!(
                // A GPR-to-XMM move, such as `movq %rax,%xmm0`.
                !body.lines().any(|line| {
                    let mut words = line.split_whitespace().skip(1);
                    matches!(words.next(), Some("movq" | "movd"))
                        && words.next().is_some_and(|operands| {
                            (operands.starts_with("%r") || operands.starts_with("%e"))
                                && operands.contains(",%xmm")
                        })
                }),
                "{name}:\n{body}"
            );
        }
    }
}

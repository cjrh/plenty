//! A numeric cast of a value known at compile time, such as `i64(u16.MAX)`
//! written in a function body, lowers to one constant of the target type
//! instead of a constant and an extension, reduction or conversion that
//! Cranelift keeps (issue #522). Results match the run-time conversions, and
//! a constant out of the target's range still traps when the cast runs.
mod support;

/// (type, bits, signed)
const TYPES: [(&str, u32, bool); 8] = [
    ("u8", 8, false),
    ("u16", 16, false),
    ("u32", 32, false),
    ("u64", 64, false),
    ("i8", 8, true),
    ("i16", 16, true),
    ("i32", 32, true),
    ("i64", 64, true),
];

fn range(bits: u32, signed: bool) -> (i128, i128) {
    if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

/// Constant expressions of each type, as Plenty and as numbers.
fn values(ty: &str, bits: u32, signed: bool) -> Vec<(String, i128)> {
    let (min, max) = range(bits, signed);
    let mut values = vec![
        (format!("{ty}.MIN"), min),
        (format!("{ty}.MAX"), max),
        (format!("({ty}.MAX // 3{ty})"), max / 3),
    ];
    if signed {
        values.push((format!("-1{ty}"), -1));
    }
    values
}

fn wrap(value: i128, bits: u32, signed: bool) -> i128 {
    let low = value.rem_euclid(1i128 << bits);
    if signed && low >= 1i128 << (bits - 1) {
        low - (1i128 << bits)
    } else {
        low
    }
}

/// Float conversions of constants, each compared with the literal it must
/// produce.
const FLOATS: [&str; 9] = [
    "f32(16777217) == 16777216f32",
    "f32(i64.MAX) == 9223372036854775808f32",
    "f64(u64.MAX) == 18446744073709551616f64",
    "f64(i8.MIN) == -128f64",
    "f32(0.1) == 0.1f32",
    "f64(0.1f32) == 0.10000000149011612f64",
    "i32(-2.75) == -2i32",
    "u8(255.9) == 255u8",
    "i64(f32(-0.5)) == 0",
];

/// Every integer conversion of each constant: the bare cast where the value
/// fits and `wrapping_from` always, then the float conversions.
fn program() -> (String, String) {
    let mut source = String::from("def main() -> Result[(), AllocError | IoError]:\n");
    let mut expected = String::new();
    for (from, from_bits, from_signed) in TYPES {
        for (text, value) in values(from, from_bits, from_signed) {
            for (to, bits, signed) in TYPES {
                let (min, max) = range(bits, signed);
                if (min..=max).contains(&value) {
                    source += &format!("    print({to}({text}))?\n");
                    expected += &format!("{value}\n");
                }
                source += &format!("    print({to}.wrapping_from({text}))?\n");
                expected += &format!("{}\n", wrap(value, bits, signed));
            }
        }
    }
    for comparison in FLOATS {
        source += &format!("    print({comparison})?\n");
        expected += "True\n";
    }
    source += "    Ok(())\n";
    (source, expected)
}

fn plenty(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: std::process::Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn constant_casts_match_run_time_conversions() {
    let dir = tempfile::tempdir().unwrap();
    let (source, expected) = program();
    std::fs::write(dir.path().join("casts.plenty"), source).unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("casts.plenty");
        assert_eq!(stdout(plenty(dir.path(), &args)), expected, "{mode:?}");
    }
}

/// Each function has a `_literal` twin that writes the cast's result.
const SHAPES: &str = "\
def widen(value: i64) -> bool:
    value > i64(u16.MAX)

def widen_literal(value: i64) -> bool:
    value > 65535

def narrow(value: u8) -> bool:
    value == u8(i64(200))

def narrow_literal(value: u8) -> bool:
    value == 200u8

def signed(value: i8) -> bool:
    value == i8.wrapping_from(u8.MAX)

def signed_literal(value: i8) -> bool:
    value == -1i8

def to_float(value: f64) -> f64:
    value * f64(u16.MAX)

def to_float_literal(value: f64) -> f64:
    value * 65535.0

def demote(value: f32) -> f32:
    value * f32(0.5)

def demote_literal(value: f32) -> f32:
    value * 0.5f32

def truncate(value: i32) -> bool:
    value == i32(-2.75)

def truncate_literal(value: i32) -> bool:
    value == -2i32

def main() -> Result[(), IoError]:
    print(widen(70000) and widen_literal(70000))?
    print(narrow(200u8) and narrow_literal(200u8))?
    print(signed(-1i8) and signed_literal(-1i8))?
    print(to_float(1.0) == to_float_literal(1.0))?
    print(demote(2.0f32) == demote_literal(2.0f32))?
    print(truncate(-2i32) and truncate_literal(-2i32))?
    Ok(())
";

/// The text of one function's section in a dump stage file.
fn section(dir: &std::path::Path, stage: &str, name: &str) -> String {
    let text = std::fs::read_to_string(dir.join("out").join(stage)).unwrap();
    let start = text
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("no `{name}` in {stage}"));
    let rest = &text[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

/// The instructions of a function's body, without value numbers.
fn instructions(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('v') || line.starts_with("return"))
        .map(|line| {
            let operation = line.split_once(" = ").map_or(line, |(_, op)| op);
            let operation = operation.split(';').next().unwrap();
            operation
                .split_whitespace()
                .filter(|word| !word.trim_end_matches(',').starts_with('v'))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn constant_casts_lower_like_their_literals() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shapes.plenty"), SHAPES).unwrap();
    let names = [
        "widen", "narrow", "signed", "to_float", "demote", "truncate",
    ];
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.extend(["--dump", "clif,clif-opt", "shapes.plenty", "-o", "out"]);
        stdout(plenty(dir.path(), &args));
        for stage in ["clif.txt", "clif-opt.txt"] {
            for name in names {
                // The unoptimized prologue before the first line marker
                // has its own extensions.
                let full = section(dir.path(), stage, name);
                let body = &full[full.find("; line").unwrap()..];
                for conversion in [
                    "extend", "ireduce", "fcvt", "fpromote", "fdemote", "brif", "trap",
                ] {
                    assert!(
                        !body.contains(conversion),
                        "{mode:?} {stage} {name}:\n{full}"
                    );
                }
            }
        }
        // Without `--release` the operand's unused constant stays in the
        // CLIF; instruction selection emits no code for it.
        for name in names.into_iter().filter(|_| !mode.is_empty()) {
            let body = section(dir.path(), "clif-opt.txt", name);
            let literal = section(dir.path(), "clif-opt.txt", &format!("{name}_literal"));
            assert_eq!(
                instructions(&body),
                instructions(&literal),
                "{mode:?} {name}:\n{body}\n{literal}"
            );
        }
        let mut args = mode.to_vec();
        args.push("shapes.plenty");
        assert_eq!(
            stdout(plenty(dir.path(), &args)),
            "True\n".repeat(6),
            "{mode:?}"
        );
    }
}

#[test]
fn constants_out_of_range_still_trap() {
    let dir = tempfile::tempdir().unwrap();
    for (case, cast) in [
        ("integer", "u8(i64(u16.MAX))"),
        ("negative", "u8(i64(-1i8))"),
        ("float", "u8(f64(256))"),
    ] {
        let file = format!("{case}.plenty");
        let source = format!(
            "def over() -> u8:\n    {cast}\n\n\
             def main() -> Result[(), IoError]:\n    print(over())?\n    Ok(())\n"
        );
        std::fs::write(dir.path().join(&file), source).unwrap();
        for mode in [&[][..], &["--release"][..]] {
            let mut args = mode.to_vec();
            args.push(&file);
            let output = plenty(dir.path(), &args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{case} {mode:?}: {stderr}");
            assert!(
                stderr.contains(&format!("error: {file}:2:5: integer cast out of range")),
                "{case} {mode:?}: {stderr}"
            );
            assert!(output.stdout.is_empty(), "{case} {mode:?}");
        }
    }
}

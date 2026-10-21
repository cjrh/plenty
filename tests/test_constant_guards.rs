//! Arithmetic guards whose condition is computed from constants and is false
//! (a literal divisor's zero test, a literal shift count's range test, a
//! constant sum's overflow test) emit no branch or trap call, while every
//! guard that can fire keeps its diagnostic and source position (issue #367).
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

/// The values each type is tested at, as Plenty expressions and as numbers.
fn boundaries(ty: &str, bits: u32, signed: bool) -> Vec<(String, i128)> {
    let (min, max) = if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    };
    let mut values = vec![
        (format!("{ty}.MIN"), min),
        (format!("{ty}.MIN + 1{ty}"), min + 1),
        (format!("0{ty}"), 0),
        (format!("1{ty}"), 1),
        (format!("9{ty}"), 9),
        (format!("{ty}.MAX - 1{ty}"), max - 1),
        (format!("{ty}.MAX"), max),
    ];
    if signed {
        values.push((format!("-1{ty}"), -1));
        values.push((format!("-9{ty}"), -9));
    }
    values
}

fn program() -> String {
    let mut source = String::from("import std.fmt\n\n");
    for (ty, bits, signed) in TYPES {
        let top = bits - 1;
        source += &format!(
            "def lit_{ty}(x: {ty}) -> {ty}:\n    \
             (x // 7{ty}) ^ (x % 10{ty}) ^ (x >> {top}{ty}) ^ (x << 1{ty})\n\n\
             def expr_{ty}(x: {ty}) -> {ty}:\n    \
             (x // (5{ty} + 5{ty})) ^ (x >> (3{ty} + 3{ty})) ^ (x % (1{ty} << 2{ty}))\n\n"
        );
        if signed {
            source += &format!(
                "def neg_{ty}(x: {ty}) -> {ty}:\n    \
                 (x // -2{ty}) ^ (x % -3{ty}) ^ (x % -1{ty})\n\n"
            );
        }
    }
    // `d` reaches the loop body as a block parameter of the unsealed loop
    // header, so its guards are decided only once the function is built.
    source += "def loop_div(x: u64) -> u64:\n    \
               d = 10u64\n    \
               mut s = 0u64\n    \
               mut i = 0u64\n    \
               while i < x:\n        \
               s = s ^ (i // d) ^ (i >> (d - 4u64))\n        \
               i = i.wrapping_add(1u64)\n    \
               s\n\n";
    source += "def main() -> Result[(), AllocError | IoError | std.fmt.FormatError]:\n";
    source += "    print(f\"{loop_div(1000u64)}\"?)?\n";
    for (ty, bits, signed) in TYPES {
        let values: Vec<String> = boundaries(ty, bits, signed)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        let neg = if signed {
            format!(" {{neg_{ty}(x)}}")
        } else {
            String::new()
        };
        source += &format!(
            "    for x in [{}]?:\n        \
             print(f\"{ty} {{x}} {{lit_{ty}(x)}} {{expr_{ty}(x)}}{neg}\"?)?\n",
            values.join(", ")
        );
    }
    source += "    Ok(())\n";
    source
}

fn floor_div(a: i128, b: i128) -> i128 {
    let q = a / b;
    if a % b != 0 && (a < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

fn floor_mod(a: i128, b: i128) -> i128 {
    let r = a % b;
    if r != 0 && (r < 0) != (b < 0) {
        r + b
    } else {
        r
    }
}

fn wrap(value: i128, bits: u32, signed: bool) -> i128 {
    let low = value.rem_euclid(1i128 << bits);
    if signed && low >= 1i128 << (bits - 1) {
        low - (1i128 << bits)
    } else {
        low
    }
}

/// The output computed independently of the compiler; a signed value's
/// right shift is arithmetic.
fn expected() -> String {
    let looped = (0..1000i128).fold(0, |s, i| s ^ (i / 10) ^ (i >> 6));
    let mut out = format!("{looped}\n");
    for (ty, bits, signed) in TYPES {
        for (_, x) in boundaries(ty, bits, signed) {
            let lit =
                floor_div(x, 7) ^ floor_mod(x, 10) ^ (x >> (bits - 1)) ^ wrap(x << 1, bits, signed);
            let expr = floor_div(x, 10) ^ (x >> 6) ^ floor_mod(x, 4);
            out += &format!("{ty} {x} {lit} {expr}");
            if signed {
                let neg = floor_div(x, -2) ^ floor_mod(x, -3) ^ floor_mod(x, -1);
                out += &format!(" {neg}");
            }
            out.push('\n');
        }
    }
    out
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
fn literal_operands_keep_arithmetic_results_at_every_width() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("guards.plenty"), program()).unwrap();
    let expected = expected();
    assert_eq!(stdout(plenty(dir.path(), &["guards.plenty"])), expected);
    assert_eq!(
        stdout(plenty(dir.path(), &["--release", "guards.plenty"])),
        expected
    );
}

const TRAPS: &str = r#"import std.env

def div(x: i64, y: i64) -> i64:
    x // y

def neg_one(x: i64) -> i64:
    x // -1

def shr(x: u32, n: u32) -> u32:
    x >> n

def bump(x: u8) -> u8:
    x + 1u8

def selector() -> Result[i64, IoError]:
    match std.env.get("CASE")?:
        case Some(text):
            Ok(len(text))
        case Nothing:
            Ok(-1)

def main() -> Result[(), AllocError | IoError]:
    choice = selector()?
    if choice == 0:
        print(7u64 // (5u64 - 5u64))?
    elif choice == 1:
        print(200u8 + 56u8)?
    elif choice == 2:
        print(1u64 << (60u64 + 4u64))?
    elif choice == 3:
        print(div(7, 0))?
    elif choice == 4:
        print(div(i64.MIN, -1))?
    elif choice == 5:
        print(neg_one(i64.MIN))?
    elif choice == 6:
        print(shr(1u32, 32u32))?
    elif choice == 7:
        print(bump(255u8))?
    else:
        print(div(i64.MIN, 1), neg_one(i64.MIN + 1), shr(8u32, 31u32), bump(254u8))?
    Ok(())
"#;

#[test]
fn guards_that_can_fire_keep_their_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("traps.plenty"), TRAPS).unwrap();
    let cases = [
        "traps.plenty:25:15: division by zero",
        "traps.plenty:27:15: integer overflow",
        "traps.plenty:29:15: shift count outside integer width",
        "traps.plenty:4:5: division by zero",
        "traps.plenty:4:5: integer overflow",
        "traps.plenty:7:5: integer overflow",
        "traps.plenty:10:5: shift count outside integer width",
        "traps.plenty:13:5: integer overflow",
    ];
    for mode in [&[][..], &["--release"][..]] {
        let executable = dir.path().join("traps");
        let mut args = mode.to_vec();
        args.extend(["--compile", "traps.plenty", "-o", "traps"]);
        stdout(plenty(dir.path(), &args));
        let run = |case: usize| {
            std::process::Command::new(&executable)
                .env("CASE", "x".repeat(case))
                .output()
                .unwrap()
        };
        for (case, message) in cases.iter().enumerate() {
            let output = run(case);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{mode:?} case {case}");
            assert!(
                stderr.contains(&format!("error: {message}")),
                "{mode:?} case {case}: {stderr}"
            );
        }
        let output = run(cases.len());
        assert!(output.status.success(), "{mode:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "-9223372036854775808 9223372036854775807 0 255\n"
        );
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
fn constant_false_guards_emit_no_branch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("guards.plenty"), program()).unwrap();
    std::fs::write(dir.path().join("traps.plenty"), TRAPS).unwrap();
    for (mode, stages) in [
        (&[][..], "clif,clif-opt"),
        (&["--release"][..], "clif,clif-opt"),
    ] {
        for file in ["guards.plenty", "traps.plenty"] {
            let mut args = mode.to_vec();
            args.extend(["--dump", stages, file, "-o", "out"]);
            stdout(plenty(dir.path(), &args));
            for stage in stages.split(',').map(|stage| format!("{stage}.txt")) {
                if file == "guards.plenty" {
                    for (ty, _, signed) in TYPES {
                        let mut names = vec![format!("lit_{ty}"), format!("expr_{ty}")];
                        if signed {
                            names.push(format!("neg_{ty}"));
                        }
                        for name in names {
                            let body = section(dir.path(), &stage, &name);
                            assert!(
                                !body.contains("plenty_trap") && !body.contains("brif"),
                                "{mode:?} {stage} {name}:\n{body}"
                            );
                        }
                    }
                    // After lowering, `loop_div` still branches to its trap
                    // calls; the built function no longer reaches them (the
                    // helpers stay declared).
                    let body = section(dir.path(), &stage, "loop_div");
                    assert_eq!(
                        body.contains("call "),
                        stage == "clif.txt",
                        "{mode:?} {stage} loop_div:\n{body}"
                    );
                    continue;
                }
                // Guards that can fire keep exactly their own trap helpers.
                for (name, kept, removed) in [
                    (
                        "div",
                        &["plenty_trap_div_zero", "plenty_trap_overflow"][..],
                        "",
                    ),
                    (
                        "neg_one",
                        &["plenty_trap_overflow"][..],
                        "plenty_trap_div_zero",
                    ),
                    ("shr", &["plenty_trap_shift"][..], ""),
                    ("bump", &["plenty_trap_overflow"][..], ""),
                ] {
                    let body = section(dir.path(), &stage, name);
                    for helper in kept {
                        assert!(body.contains(helper), "{mode:?} {stage} {name}:\n{body}");
                    }
                    assert!(
                        removed.is_empty() || !body.contains(removed),
                        "{mode:?} {stage} {name}:\n{body}"
                    );
                }
            }
        }
    }
}

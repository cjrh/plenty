//! Checked addition and subtraction trap exactly when the mathematical result
//! leaves the type's range, for every integer width, with variable and
//! constant operands, in debug and release builds. Debug code tests most of
//! these with an `icmp` on the wrapped result rather than an overflow flag
//! (issue #425), so the boundaries are checked against exact arithmetic.
mod support;
use rstest::rstest;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

struct Int {
    name: &'static str,
    min: i128,
    max: i128,
}

const TYPES: [Int; 8] = [
    Int {
        name: "i8",
        min: i8::MIN as i128,
        max: i8::MAX as i128,
    },
    Int {
        name: "i16",
        min: i16::MIN as i128,
        max: i16::MAX as i128,
    },
    Int {
        name: "i32",
        min: i32::MIN as i128,
        max: i32::MAX as i128,
    },
    Int {
        name: "i64",
        min: i64::MIN as i128,
        max: i64::MAX as i128,
    },
    Int {
        name: "u8",
        min: 0,
        max: u8::MAX as i128,
    },
    Int {
        name: "u16",
        min: 0,
        max: u16::MAX as i128,
    },
    Int {
        name: "u32",
        min: 0,
        max: u32::MAX as i128,
    },
    Int {
        name: "u64",
        min: 0,
        max: u64::MAX as i128,
    },
];

/// A checked function of one or two operands: its name suffix, Plenty body
/// and the exact result.
struct Function {
    suffix: String,
    body: String,
    binary: bool,
    exact: Box<dyn Fn(i128, i128) -> i128>,
}

fn functions(ty: &Int) -> Vec<Function> {
    let mut functions = vec![
        Function {
            suffix: "add".into(),
            body: "a + b".into(),
            binary: true,
            exact: Box::new(|a, b| a + b),
        },
        Function {
            suffix: "sub".into(),
            body: "a - b".into(),
            binary: true,
            exact: Box::new(|a, b| a - b),
        },
    ];
    let mut constants = vec![("zero", 0), ("one", 1), ("max", ty.max)];
    if ty.min < 0 {
        // The most negative literal is spelled as an expression, which the
        // compiler folds to a constant.
        constants.extend([("minus_one", -1), ("min", ty.min)]);
    }
    for (label, c) in constants {
        let literal = if c < 0 && c == ty.min {
            format!("({} - 1)", -ty.max)
        } else if c < 0 {
            format!("({c})")
        } else {
            c.to_string()
        };
        functions.push(Function {
            suffix: format!("add_{label}"),
            body: format!("a + {literal}"),
            binary: false,
            exact: Box::new(move |a, _| a + c),
        });
        functions.push(Function {
            suffix: format!("{label}_add"),
            body: format!("{literal} + a"),
            binary: false,
            exact: Box::new(move |a, _| c + a),
        });
        functions.push(Function {
            suffix: format!("sub_{label}"),
            body: format!("a - {literal}"),
            binary: false,
            exact: Box::new(move |a, _| a - c),
        });
        functions.push(Function {
            suffix: format!("{label}_sub"),
            body: format!("{literal} - a"),
            binary: false,
            exact: Box::new(move |a, _| c - a),
        });
    }
    functions
}

fn samples(ty: &Int) -> Vec<i128> {
    let mut values = vec![ty.min, ty.min + 1, 0, 1, ty.max - 1, ty.max];
    if ty.min < 0 {
        values.push(-1);
    }
    values
}

/// One program defining every function, which runs the one `CASE` names on
/// operands parsed from `A` and `B`, so no operand is a compile-time constant.
fn program() -> String {
    let mut source = String::new();
    let mut dispatch = String::new();
    for ty in &TYPES {
        let t = ty.name;
        for function in functions(ty) {
            let name = format!("{t}_{}", function.suffix);
            let (parameters, arguments) = if function.binary {
                (
                    format!("a: {t}, b: {t}"),
                    format!("{t}.parse(&a).unwrap(), {t}.parse(&b).unwrap()"),
                )
            } else {
                (format!("a: {t}"), format!("{t}.parse(&a).unwrap()"))
            };
            writeln!(
                source,
                "def {name}({parameters}) -> {t}:\n    {}\n",
                function.body
            )
            .unwrap();
            let keyword = if dispatch.is_empty() { "if" } else { "elif" };
            writeln!(
                dispatch,
                "    {keyword} name == \"{name}\":\n        print({name}({arguments}))?"
            )
            .unwrap();
        }
    }
    format!(
        "import std.env\n\n{source}def main() -> Result[(), AllocError | IoError]:\n    \
         name = std.env.get(\"CASE\")?.unwrap()\n    \
         a = std.env.get(\"A\")?.unwrap()\n    \
         b = std.env.get(\"B\")?.unwrap()\n{dispatch}    Ok(())\n"
    )
}

fn compile(dir: &Path, release: bool) -> std::path::PathBuf {
    let source = dir.join("checked.plenty");
    let executable = dir.join("checked");
    std::fs::write(&source, program()).unwrap();
    let mut command = Command::new(support::plenty_bin());
    command.env("PLENTY_CACHE", "off");
    if release {
        command.arg("--release");
    }
    let output = command
        .arg("--compile")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

#[rstest]
#[case(false)]
#[case(true)]
fn add_and_sub_trap_exactly_outside_the_range(#[case] release: bool) {
    let dir = tempfile::tempdir().unwrap();
    let executable = compile(dir.path(), release);
    for ty in &TYPES {
        let values = samples(ty);
        for function in functions(ty) {
            let name = format!("{}_{}", ty.name, function.suffix);
            let pairs: Vec<(i128, i128)> = if function.binary {
                values
                    .iter()
                    .flat_map(|&a| values.iter().map(move |&b| (a, b)))
                    .collect()
            } else {
                values.iter().map(|&a| (a, 0)).collect()
            };
            for (a, b) in pairs {
                let output = Command::new(&executable)
                    .env("CASE", &name)
                    .env("A", a.to_string())
                    .env("B", b.to_string())
                    .output()
                    .unwrap();
                let exact = (function.exact)(a, b);
                let context = format!("{name}({a}, {b}) release={release}");
                if (ty.min..=ty.max).contains(&exact) {
                    assert!(output.status.success(), "{context}");
                    assert_eq!(
                        String::from_utf8_lossy(&output.stdout),
                        format!("{exact}\n"),
                        "{context}"
                    );
                } else {
                    assert_eq!(output.status.code(), Some(1), "{context}");
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    assert!(
                        stderr.starts_with("error: checked.plenty:")
                            && stderr.ends_with(": integer overflow\n"),
                        "{context}: {stderr}"
                    );
                }
            }
        }
    }
}

/// Debug code checks an add of a constant with `cmp; jl` instead of
/// materializing the overflow flag with `seto; test; jne` (issue #425).
#[test]
fn constant_add_compares_instead_of_materializing_the_flag() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("constant.plenty");
    std::fs::write(
        &source,
        "def step(x: i64) -> i64:\n    x + 7\n\ndef main() -> i32:\n    i32(step(3))\n",
    )
    .unwrap();
    let output = Command::new(support::plenty_bin())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "asm", "--dump-filter", "step"])
        .arg(&source)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = String::from_utf8(output.stdout).unwrap();
    assert!(!asm.contains("seto"), "{asm}");
    let branch = if cfg!(target_arch = "aarch64") {
        "b.lt "
    } else {
        "jl "
    };
    assert!(asm.contains(branch), "{asm}");
}

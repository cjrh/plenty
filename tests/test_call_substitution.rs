//! Reads of module constants become literals of the constant's type, and in
//! release builds calls to tiny leaf predicates run in the caller (issue
//! #363). Results, traps and their positions are those of the calls.
mod support;

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

/// The text of one function's section in a dump stage file, if dumped.
fn section(dir: &std::path::Path, stage: &str, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("out").join(stage)).unwrap();
    let start = text.find(&format!("==== {name} "))?;
    let rest = &text[start + 5..];
    Some(rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string())
}

fn dump(dir: &std::path::Path, mode: &[&str], file: &str) {
    let mut args = mode.to_vec();
    args.extend(["--dump", "ops,asm", "--dump-std", file, "-o", "out"]);
    stdout(plenty(dir, &args));
}

const LIMITS: &str = "\
pub const top: u64 = u64.MAX
pub const bottom: i64 = i64.MIN
pub const small: i8 = -128
pub const byte: u8 = u8.MAX
pub const third: f32 = 0.33333334
pub const cold: f64 = -f64.INF
pub const width: i64 = 8
";

/// `width` is defined in both modules with different types and values.
const MAIN: &str = "\
import limits as l
from limits import top as most, width as other_width
const width: i16 = -3

def scaled(n: i64) -> i64:
    n * other_width + l.width

def main() -> Result[(), IoError]:
    print(most)?
    print(l.bottom)?
    print(l.small)?
    print(l.byte)?
    print(l.third)?
    print(l.cold)?
    print(width)?
    print(scaled(5))?
    print(most - 1)?
    Ok(())
";

const EXPECTED: &str = "18446744073709551615\n-9223372036854775808\n-128\n255\n0.33333334\n-inf\n-3\n48\n18446744073709551614\n";

#[test]
fn constants_keep_their_values_types_and_bindings_without_getters() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("limits.plenty"), LIMITS).unwrap();
    std::fs::write(dir.path().join("main.plenty"), MAIN).unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("main.plenty");
        assert_eq!(stdout(plenty(dir.path(), &args)), EXPECTED, "{mode:?}");
        dump(dir.path(), mode, "main.plenty");
        let index = std::fs::read_to_string(dir.path().join("out/index.txt")).unwrap();
        assert!(!index.contains("$const."), "{mode:?}: {index}");
        // The retained caller reads `limits.width` as an immediate.
        let scaled = section(dir.path(), "ops.txt", "scaled").unwrap();
        assert!(scaled.contains("PushInt(I64(8))"), "{scaled}");
        assert!(!scaled.contains("Call("), "{scaled}");
        let main = section(dir.path(), "ops.txt", "main").unwrap();
        for literal in [
            "PushInt(U64(18446744073709551615))",
            "PushInt(I64(-9223372036854775808))",
            "PushInt(I8(-128))",
            "PushInt(U8(255))",
            "PushInt(I16(-3))",
        ] {
            assert!(main.contains(literal), "{mode:?}: {literal}\n{main}");
        }
        assert!(!main.contains("$const"), "{main}");
    }
}

/// Folded expressions, `str` and `bytes` constants, and included files are
/// read as literals too: no getter survives and no code runs to compute them.
#[test]
fn folded_text_and_included_constants_have_no_getters() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "noted").unwrap();
    std::fs::write(
        dir.path().join("main.plenty"),
        "\
const KIB: i64 = 1 << 10
const MIB: i64 = KIB * KIB
const NAME: str = \"plenty\" + \"-\" + \"lang\"
const MAGIC: bytes = b\"\\x7fELF\"
const NOTE: str = include_str(\"note.txt\")
assert MIB == 1048576

def main() -> Result[(), IoError]:
    print(MIB)?
    print(NAME)?
    print(MAGIC)?
    print(NOTE)?
    Ok(())
",
    )
    .unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("main.plenty");
        assert_eq!(
            stdout(plenty(dir.path(), &args)),
            "1048576\nplenty-lang\nb\"\\x7fELF\"\nnoted\n",
            "{mode:?}"
        );
        dump(dir.path(), mode, "main.plenty");
        let index = std::fs::read_to_string(dir.path().join("out/index.txt")).unwrap();
        assert!(!index.contains("$const."), "{mode:?}: {index}");
        assert!(!index.contains("$assert"), "{mode:?}: {index}");
        let main = section(dir.path(), "ops.txt", "main").unwrap();
        assert!(main.contains("PushInt(I64(1048576))"), "{mode:?}: {main}");
        assert!(!main.contains("$const"), "{main}");
    }
}

/// `digit` and `blank` are leaf predicates; `bump` also substitutes its checked
/// arithmetic in optimized native code. Recursive `countdown` stays a call.
const PREDICATES: &str = "\
const space: u8 = 32
const tab: u8 = 9

def digit(code: i64) -> bool:
    code >= 48 and code <= 57

def blank(value: u8) -> bool:
    value == space or value == tab

def bump(code: i64) -> i64:
    code + 1

def countdown(n: i64) -> bool:
    if n == 0:
        return True
    countdown(n - 1)

def classify(code: i64, gap: u8) -> i64:
    mut kind = 0
    if digit(code):
        kind = 1
    if blank(gap) and not digit(code + 1):
        kind = kind + 2
    kind

def check(code: i64, divisor: i64) -> i64:
    if digit(code):
        return code // divisor
    0

def main() -> Result[(), IoError]:
    print(classify(48, 32))?
    print(classify(57, 9))?
    print(classify(47, 10))?
    print(bump(1))?
    print(countdown(100000))?
    print(check(50, 2))?
    print(check(50, 0))?
    Ok(())
";

#[test]
fn leaf_predicates_run_in_the_caller_in_release_builds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), PREDICATES).unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("p.plenty");
        let output = plenty(dir.path(), &args);
        assert_eq!(output.status.code(), Some(1), "{mode:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "1\n3\n0\n2\nTrue\n25\n",
            "{mode:?}"
        );
        // The trap after an inlined call keeps the caller's position.
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "error: p.plenty:28:16: division by zero\n",
            "{mode:?}"
        );
    }
    dump(dir.path(), &[], "p.plenty");
    let classify = section(dir.path(), "asm.txt", "classify").unwrap();
    assert!(classify.contains("__plenty_fn_digit"), "{classify}");

    dump(dir.path(), &["--release"], "p.plenty");
    for caller in ["classify", "check"] {
        let asm = section(dir.path(), "asm.txt", caller).unwrap();
        let code = asm.split_once('\n').unwrap().1;
        assert!(!code.contains("__plenty_fn_"), "{caller}:\n{asm}");
    }
    let main = section(dir.path(), "asm.txt", "main").unwrap();
    assert!(!main.contains("__plenty_fn_bump\n"), "{main}");
    for callee in ["countdown", "classify", "check"] {
        assert!(
            main.contains(&format!("__plenty_fn_{callee}\n")),
            "{callee}:\n{main}"
        );
    }
    let countdown = section(dir.path(), "ops.txt", "countdown").unwrap();
    assert!(countdown.contains("TailCall(\"countdown\")"), "{countdown}");
    assert!(section(dir.path(), "asm.txt", "digit").is_some());
}

/// A standard-library constant read by user code is a literal there.
#[test]
fn standard_constants_are_literals_in_user_code() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("s.plenty"),
        "\
from std.math import pi

def area(r: f64) -> f64:
    pi * r * r

def main() -> Result[(), IoError]:
    print(area(2.0))?
    Ok(())
",
    )
    .unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("s.plenty");
        assert_eq!(stdout(plenty(dir.path(), &args)), "12.566370614359172\n");
        dump(dir.path(), mode, "s.plenty");
        let area = section(dir.path(), "asm.txt", "area").unwrap();
        let code = area.split_once('\n').unwrap().1;
        assert!(!code.contains("__plenty_fn_"), "{mode:?}:\n{area}");
    }
}

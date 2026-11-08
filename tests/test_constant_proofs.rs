//! An integer module constant proves what the literal it folds to proves
//! (issue #863): as a divisor of `//` or `%`, as a shift count, and as the
//! mask in `n & L`. Constants that could fail keep their errors, and locals,
//! guards and arithmetic still prove nothing.
mod support;
use std::process::Command;
use support::{check_diagnostic, check_source, run_stdout};

/// (type, bits, signed)
const WIDTHS: [(&str, u32, bool); 8] = [
    ("i8", 8, true),
    ("i16", 16, true),
    ("i32", 32, true),
    ("i64", 64, true),
    ("u8", 8, false),
    ("u16", 16, false),
    ("u32", 32, false),
    ("u64", 64, false),
];

/// The same program with literals or with constants that fold to them. No
/// function declares an error, so each operand must prove itself.
fn paired_program(constants: bool) -> String {
    let mut source = String::new();
    let mut main = String::from("def main() -> Result[(), IoError]:\n");
    for (ty, bits, signed) in WIDTHS {
        let last = bits - 1;
        let (d, n, s, m) = if constants {
            (
                format!("d_{ty}"),
                format!("n_{ty}"),
                format!("s_{ty}"),
                format!("m_{ty}"),
            )
        } else {
            ("3".into(), "-3".into(), last.to_string(), last.to_string())
        };
        source.push_str(&format!(
            "def quotient_{ty}(x: {ty}) -> ({ty}, {ty}):\n    (x // {d}, x % {d})\n\
             def compound_{ty}(x: {ty}) -> {ty}:\n    mut y = x\n    y //= {d}\n    y %= {d}\n    y\n\
             def shifts_{ty}(x: {ty}, n: {ty}) -> ({ty}, {ty}):\n    (x << {s}, x >> (n & {m}))\n"
        ));
        main.push_str(&format!(
            "    print(quotient_{ty}({ty}.MIN), quotient_{ty}({ty}.MAX), compound_{ty}({ty}.MAX))?\n\
             \x20   print(shifts_{ty}(1{ty}, {ty}.MAX), shifts_{ty}({ty}.MAX, 1{ty}))?\n"
        ));
        if signed {
            source.push_str(&format!(
                "def negative_{ty}(x: {ty}) -> ({ty}, {ty}):\n    (x // {n}, x % {n})\n"
            ));
            main.push_str(&format!(
                "    print(negative_{ty}({ty}.MIN), negative_{ty}({ty}.MAX))?\n"
            ));
        }
        // Declared after their uses; the divisor is a folded expression.
        if constants {
            source.push_str(&format!(
                "const d_{ty}: {ty} = 1 + 2\nconst s_{ty}: {ty} = {last}\nconst m_{ty}: {ty} = s_{ty}\n"
            ));
            if signed {
                source.push_str(&format!("const n_{ty}: {ty} = -3\n"));
            }
        }
    }
    main.push_str("    Ok(())\n");
    source + &main
}

#[test]
fn constants_prove_what_the_same_literal_proves_at_every_width() {
    let literal = run_stdout(&paired_program(false));
    assert_eq!(run_stdout(&paired_program(true)), literal);
    assert!(
        literal.starts_with("(-43, 1) (42, 1) 0\n(-128, 0) (-128, 63)\n(42, -2) (-43, -2)\n"),
        "{literal}"
    );
}

#[test]
fn constants_that_can_fail_keep_their_errors() {
    for (body, error) in [
        ("x // zero", "ZeroDivisionError"),
        ("x % zero", "ZeroDivisionError"),
        ("x // minus_one", "OverflowError[i64]"),
        (
            "mut y = x\n    y //= minus_one\n    y",
            "OverflowError[i64]",
        ),
        // A negated constant and a local are arithmetic and values, not
        // constants.
        ("x // -two", "OverflowError[i64]"),
        ("d = two\n    x // d", "ZeroDivisionError"),
        ("x << count", "ShiftCountError[i64]"),
        ("x << (count & wide)", "ShiftCountError[i64]"),
    ] {
        let message = check_diagnostic(&format!(
            "const zero: i64 = 0\nconst minus_one: i64 = -1\nconst two: i64 = 2\nconst wide: i64 = 64\n\
             def f(x: i64, count: i64) -> i64:\n    {body}\ndef main() -> ():\n    pass\n"
        ));
        assert!(
            message.contains(&format!("arithmetic can fail with {error}")),
            "{body}: {message}"
        );
    }
    // `-1` keeps only the quotient overflow, and `MIN % -1` cannot fail.
    assert_eq!(
        run_stdout(
            "const minus_one: i64 = -1
def quotient(x: i64) -> Result[i64, OverflowError[i64]]:
    Ok(x // minus_one)
def remainder(x: i64) -> i64:
    x % minus_one
def main() -> Result[(), IoError]:
    print(quotient(i64.MIN).is_err(), quotient(5).unwrap(), remainder(i64.MIN))?
    Ok(())
"
        ),
        "True -5 0\n"
    );
    for (declaration, message) in [
        (
            "const count: u8 = 8",
            "shift count 8 is outside 0..8 for u8",
        ),
        (
            "const count: i8 = -1",
            "shift count -1 is outside 0..8 for i8",
        ),
    ] {
        let ty = &declaration[13..15];
        let diagnostic = check_diagnostic(&format!(
            "{declaration}\ndef f(x: {ty}) -> {ty}:\n    x << count\ndef main() -> ():\n    pass\n"
        ));
        assert!(diagnostic.contains(message), "{diagnostic}");
    }
}

#[test]
fn proof_values_keep_their_full_range() {
    // Neither extreme narrows to zero or -1 while it is evaluated.
    assert_eq!(
        run_stdout(
            "const huge: u64 = u64.MAX
const lowest: i64 = i64.MIN
def f(x: u64, y: i64) -> (u64, u64, i64, i64):
    (x // huge, x % huge, y // lowest, y % lowest)
def main() -> Result[(), IoError]:
    print(f(u64.MAX, i64.MIN), f(5u64, 5))?
    Ok(())
"
        ),
        "(1, 0, 1, 0) (0, 5, -1, -9223372036854775803)\n"
    );
}

#[test]
fn checked_blocks_and_generic_instances_infer_from_constants() {
    assert_eq!(
        run_stdout(
            "const two: i64 = 2
const shift: u32 = 4
def mean(a: i64, b: i64) -> i64:
    result = checked:
        (a + b) // two
    match result:
        case Ok(value):
            value
        case Err(OverflowError(wrapped)):
            wrapped // two
def scaled[T: IntType](x: i64, tag: T) -> (i64, T):
    (x // two, tag)
def main() -> Result[(), IoError]:
    print(mean(3, 5), mean(i64.MAX, 1), scaled(7, 1u8), 1u32 << shift)?
    Ok(())
"
        ),
        "4 -4611686018427387904 (3, 1) 16\n"
    );
    for body in ["x // two", "x << shift"] {
        let ty = if body.contains("shift") { "u32" } else { "i64" };
        let message = check_diagnostic(&format!(
            "const two: i64 = 2\nconst shift: u32 = 4\ndef f(x: {ty}) -> {ty}:\n    result = checked:\n        {body}\n    result.unwrap()\ndef main() -> ():\n    pass\n"
        ));
        assert!(
            message.contains("this `checked:` block contains no arithmetic that can fail"),
            "{body}: {message}"
        );
    }
    check_source(
        "const two: i64 = 2\ndef f(x: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:\n    Ok(x // two)\ndef main() -> ():\n    pass\n",
    )
    .unwrap();
}

#[test]
fn imported_and_aliased_constants_prove_divisors_and_counts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sizes.plenty"),
        "pub const step: i64 = 4\npub const bits: u32 = 5\npub const low: u32 = 3\n",
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        "import sizes
from sizes import step as s
from sizes import bits
def split(x: i64) -> (i64, i64):
    (x // sizes.step, x % s)
def pack(x: u32, n: u32) -> u32:
    (x << bits) | (x >> (n & sizes.low))
def main() -> Result[(), IoError]:
    print(split(-9), pack(8u32, 255u32))?
    Ok(())
",
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"(-3, 3) 257\n");
}

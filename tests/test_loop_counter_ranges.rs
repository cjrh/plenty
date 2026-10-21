//! Loop counter ranges (issue #369): a `while` counter's step that cannot
//! overflow lowers to a plain add, and floor modulo or division of a counter
//! that only grows from a nonnegative start needs no sign correction. Every
//! other check stays, and results match the language's checked, floored
//! semantics in debug and release builds.
use rstest::rstest;
use std::path::Path;
use std::process::{Command, Output};

const PROGRAM: &str = r#"def up_to(start: i64, limit: i64) -> i64:
    mut i = start
    while i < limit:
        i = i + 1
    i

def up_to_i8(start: i8, limit: i8) -> i8:
    mut i = start
    while i < limit:
        i += 1i8
    i

def up_to_u8(start: u8, limit: u8) -> u8:
    mut i = start
    while limit > i:
        i = 1u8 + i
    i

def through_const_i16(start: i16) -> i16:
    mut i = start
    while i <= 32766i16:
        i += 1i16
    i

def by_three_const_i32(start: i32) -> i32:
    mut i = start
    while i < 2147483645i32:
        i += 3i32
    i

def down_to_i8(start: i8, floor: i8) -> i8:
    mut i = start
    while i > floor:
        i -= 1i8
    i

def down_to_u16(start: u16) -> u16:
    mut i = start
    while i > 0u16:
        i = i - 1u16
    i

def down_through_const_u32(start: u32) -> u32:
    mut i = start
    while i >= 2u32:
        i -= 2u32
    i

def moving_limit(start: i64, limit: i64) -> i64:
    mut i = start
    mut n = limit
    while i < n:
        n = n - 1
        i += 1
    (i - start) * 10 + (limit - n)

def steps_and_skips(limit: i64) -> i64:
    mut i = 0
    mut total = 0
    while i < limit:
        i += 1
        if i % 2 == 0:
            continue
        if i > 9:
            break
        total += i
    total * 100 + i

def nested_mod(rows: i64, cols: i64) -> i64:
    mut total = 0
    mut r = 0
    while r < rows:
        mut c = 0
        while c < cols:
            total += (r * 10 + c) % 7 + c % 3 + c // 2
            c += 1
        r += 1
    total

def mod_widths() -> i64:
    mut total = 0
    mut a = 0i8
    while a < 127i8:
        total += i64(a % 7i8) + i64(a // 9i8)
        a += 1i8
    mut b = 0i16
    while b < 1000i16:
        total += i64(b % 255i16)
        b += 3i16
    mut c = 0i32
    while c < 100i32:
        total += i64(c % 13i32) + i64(c // 4i32)
        c += 1i32
    mut d = 0u64
    while d < 50u64:
        total += i64(d % 6u64)
        d += 1u64
    total

def mod_negative_divisor(limit: i64) -> i64:
    mut total = 0
    mut i = 0
    while i < limit:
        total = total * 3 + i % -4 + i // -3
        i += 1
    total

def mod_variable_divisor(limit: i64, d: i64) -> i64:
    mut total = 0
    mut i = 0
    while i < limit:
        total = total * 3 + i % d + i // d
        i += 1
    total

def mod_negative_start() -> i64:
    mut total = 0
    mut i = -7
    while i < 7:
        total = total * 3 + i % 4 + i // 4
        i += 1
    total

def mod_after_decrease() -> i64:
    mut total = 0
    mut i = 5
    while i > -6:
        total = total * 3 + i % 4 + i // 4
        i -= 1
    total

def mod_variable_step(step: i64) -> i64:
    mut total = 0
    mut i = 5
    mut n = 0
    while n < 10:
        total = total * 3 + i % 4
        i = i + step
        n += 1
    total

def mod_parameter_start(start: i64) -> i64:
    mut total = 0
    mut i = start
    while i < 5:
        total = total * 3 + i % 4
        i += 1
    total

def mod_after_reset() -> i64:
    mut total = 0
    mut i = 0
    mut reset = True
    while i < 6:
        total = total * 3 + i % 4
        i += 1
        if i == 3 and reset:
            i = -9
            reset = False
    total

def main() -> Result[(), AllocError | IoError]:
    print(up_to(0, 0), up_to(0, -5), up_to(0, 1), up_to(-3, 4), up_to(9223372036854775804, i64.MAX))?
    print(up_to_i8(0i8, i8.MIN), up_to_i8(120i8, i8.MAX), up_to_i8(-128i8, -125i8))?
    print(up_to_u8(250u8, u8.MAX), up_to_u8(7u8, 0u8))?
    print(through_const_i16(32760i16), by_three_const_i32(2147483640i32))?
    print(down_to_i8(-125i8, i8.MIN), down_to_i8(3i8, 5i8), down_to_u16(5u16), down_through_const_u32(7u32))?
    print(moving_limit(0, 10), moving_limit(9223372036854775806, i64.MAX))?
    print(steps_and_skips(100), steps_and_skips(4))?
    print(nested_mod(5, 6), mod_widths())?
    print(mod_negative_divisor(8), mod_variable_divisor(8, -3), mod_variable_divisor(8, 5))?
    print(mod_negative_start(), mod_after_decrease())?
    print(mod_variable_step(-1), mod_variable_step(2), mod_parameter_start(-6))?
    print(mod_after_reset())?
    Ok(())
"#;

const EXPECTED: &str = "0 0 1 4 9223372036854775807\n\
0 127 -125\n\
255 7\n\
32767 2147483646\n\
-128 3 0 1\n\
55 11\n\
2511 404\n\
147 44487\n\
-3922 -2900 1584\n\
-1464786 162754\n\
28047 44286 181575\n\
87169609\n\
";

fn plenty_output(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

fn plenty(dir: &Path, args: &[&str]) -> String {
    let output = plenty_output(dir, args);
    assert!(
        output.status.success(),
        "plenty {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn write(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("counters.plenty"), source).unwrap();
    dir
}

#[test]
fn loop_counters_keep_checked_and_floored_semantics() {
    let dir = write(PROGRAM);
    assert_eq!(plenty(dir.path(), &["counters.plenty"]), EXPECTED);
    assert_eq!(
        plenty(dir.path(), &["--release", "counters.plenty"]),
        EXPECTED
    );
}

/// Steps the analysis cannot prove keep their trap, at the same site.
#[rstest]
#[case::inclusive_variable_bound(
    "def f(limit: i64) -> i64:\n    mut i = limit - 2\n    while i <= limit:\n        i += 1\n    i\n",
    "f(i64.MAX)",
    "4:9: integer overflow"
)]
#[case::wide_step_variable_bound(
    "def f(limit: i64) -> i64:\n    mut i = limit - 1\n    while i < limit:\n        i += 2\n    i\n",
    "f(i64.MAX)",
    "4:9: integer overflow"
)]
#[case::wide_step_constant_bound(
    "def f(start: i8) -> i8:\n    mut i = start\n    while i < 127i8:\n        i += 2i8\n    i\n",
    "i64(f(126i8))",
    "4:9: integer overflow"
)]
#[case::inclusive_constant_maximum(
    "def f(start: u8) -> u8:\n    mut i = start\n    while i <= 255u8:\n        i += 1u8\n    i\n",
    "i64(f(250u8))",
    "4:9: integer overflow"
)]
#[case::earlier_conditional_write(
    "def f(limit: i64, bump: bool) -> i64:\n    mut i = limit - 1\n    while i < limit:\n        if bump:\n            i += 1\n        i += 1\n    i\n",
    "f(i64.MAX, True)",
    "6:9: integer overflow"
)]
#[case::second_step(
    "def f(limit: i64) -> i64:\n    mut i = limit - 1\n    while i < limit:\n        i += 1\n        i += 1\n    i\n",
    "f(i64.MAX)",
    "5:9: integer overflow"
)]
#[case::inclusive_lower_bound(
    "def f(floor: i8) -> i8:\n    mut i = floor + 2i8\n    while i >= floor:\n        i -= 1i8\n    i\n",
    "i64(f(i8.MIN))",
    "4:9: integer overflow"
)]
#[case::nested_loop_step(
    "def f(limit: i64) -> i64:\n    mut i = limit - 1\n    while i < limit:\n        mut j = 0\n        while j < 2:\n            i = i + 1\n            j += 1\n    i\n",
    "f(i64.MAX)",
    "6:17: integer overflow"
)]
#[case::zero_divisor(
    "def f(divisor: i64) -> i64:\n    mut i = 0\n    mut total = 0\n    while i < 3:\n        total += i % divisor\n        i += 1\n    total\n",
    "f(0)",
    "5:18: division by zero"
)]
fn unproven_loop_checks_still_trap(
    #[case] function: &str,
    #[case] call: &str,
    #[case] message: &str,
) {
    let source = format!(
        "{function}\ndef main() -> Result[(), AllocError | IoError]:\n    print({call})?\n    Ok(())\n"
    );
    let dir = write(&source);
    for args in [
        &["counters.plenty"][..],
        &["--release", "counters.plenty"][..],
    ] {
        let output = plenty_output(dir.path(), args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}\n{source}");
        assert!(stderr.contains(message), "{args:?}: {stderr}\n{source}");
    }
}

const SHAPES: &str = r#"def bounded_step(limit: i64) -> i64:
    mut i = 0
    mut total = 0
    while i < limit:
        total = total + i
        i = i + 1
    total

def wide_step(limit: i64) -> i64:
    mut i = 0
    while i < limit:
        i = i + 2
    i

def nonnegative_mod(limit: i64) -> i64:
    mut i = 0
    mut total = 0
    while i < limit:
        total = total.wrapping_add(i % 7 + i // 5)
        i += 1
    total

def signed_mod(limit: i64) -> i64:
    mut i = -1
    mut total = 0
    while i < limit:
        total = total.wrapping_add(i % 7 + i // 5)
        i += 1
    total

def main() -> ():
    x = bounded_step(3) + wide_step(3) + nonnegative_mod(3) + signed_mod(3)
"#;

fn clif(dir: &Path, function: &str) -> String {
    plenty(
        dir,
        &[
            "--release",
            "--dump",
            "clif",
            "--dump-filter",
            function,
            "counters.plenty",
            "-o",
            "-",
        ],
    )
}

/// The proved step loses its overflow check while the accumulator's add in
/// the same loop keeps one; a step of two below a variable bound keeps its
/// check.
#[test]
fn proven_counter_steps_lower_to_plain_adds() {
    let dir = write(SHAPES);
    let proven = clif(dir.path(), "bounded_step");
    assert_eq!(proven.matches("sadd_overflow").count(), 1, "{proven}");
    assert_eq!(
        proven.matches("plenty_trap_overflow").count(),
        1,
        "{proven}"
    );
    let wide = clif(dir.path(), "wide_step");
    assert_eq!(wide.matches("sadd_overflow").count(), 1, "{wide}");
}

/// A counter that starts at zero and only grows is divided unsigned, with no
/// floor correction; one that starts negative keeps signed division.
#[test]
fn nonnegative_counters_use_unsigned_division() {
    let dir = write(SHAPES);
    let nonnegative = clif(dir.path(), "nonnegative_mod");
    for expected in ["urem", "udiv"] {
        assert!(nonnegative.contains(expected), "{nonnegative}");
    }
    for absent in ["srem", "sdiv", "select"] {
        assert!(!nonnegative.contains(absent), "{nonnegative}");
    }
    let signed = clif(dir.path(), "signed_mod");
    for expected in ["srem", "sdiv"] {
        assert!(signed.contains(expected), "{signed}");
    }
}

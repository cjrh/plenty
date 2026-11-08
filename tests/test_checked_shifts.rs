//! Recoverable shift counts and float `try_from` (issue #862).
mod support;
use support::{assert_trap_cases, check_diagnostic, run_stdout};

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

/// Every width rejects counts at and past the width, and signed widths also
/// reject negative counts; the error reports the count, not a shifted value.
#[test]
fn every_width_reports_the_invalid_count() {
    let mut source = String::new();
    let mut expected = String::new();
    for (ty, width, signed) in WIDTHS {
        source.push_str(&format!(
            "def shl_{ty}(value: {ty}, count: {ty}) -> Result[{ty}, ShiftCountError[{ty}]]:\n    Ok(value << count)\n\
             def shr_{ty}(value: {ty}, count: {ty}) -> Result[{ty}, ShiftCountError[{ty}]]:\n    Ok(value >> count)\n\
             def count_{ty}(result: Result[{ty}, ShiftCountError[{ty}]]) -> {ty}:\n    match result:\n        case Ok(_):\n            {ty}(0)\n        case Err(ShiftCountError(count)):\n            count\n"
        ));
        let last = width - 1;
        let mut lines = vec![
            format!(
                "    print(shl_{ty}({ty}(1), {ty}({last})).is_ok(), shr_{ty}({ty}(1), {ty}({last})).unwrap())?"
            ),
            format!(
                "    print(count_{ty}(shl_{ty}({ty}(1), {ty}({width}))), count_{ty}(shr_{ty}({ty}(1), {ty}({width}))))?"
            ),
            format!(
                "    print(count_{ty}(shl_{ty}({ty}(1), {ty}({}))))?",
                width + 1
            ),
        ];
        expected.push_str(&format!("True 0\n{width} {width}\n{}\n", width + 1));
        if signed {
            lines.push(format!(
                "    print(count_{ty}(shl_{ty}({ty}(1), {ty}(-1))), count_{ty}(shr_{ty}({ty}(-8), {ty}.MIN)))?"
            ));
            let min = -(1i128 << (width - 1));
            expected.push_str(&format!("-1 {min}\n"));
        } else {
            lines.push(format!(
                "    print(count_{ty}(shl_{ty}({ty}(1), {ty}.MAX)))?"
            ));
            let max = (1u128 << width) - 1;
            expected.push_str(&format!("{max}\n"));
        }
        source.push_str(&format!(
            "def run_{ty}() -> Result[(), IoError]:\n{}\n    Ok(())\n",
            lines.join("\n")
        ));
    }
    source.push_str("def main() -> Result[(), IoError]:\n");
    for (ty, _, _) in WIDTHS {
        source.push_str(&format!("    run_{ty}()?\n"));
    }
    source.push_str("    Ok(())\n");
    assert_eq!(run_stdout(&source), expected);
}

#[test]
fn valid_shifts_keep_their_bit_semantics() {
    assert_eq!(
        run_stdout(
            r#"
def shl(value: i64, count: i64) -> Result[i64, ShiftCountError[i64]]:
    Ok(value << count)
def shr(value: i64, count: i64) -> Result[i64, ShiftCountError[i64]]:
    Ok(value >> count)
def ushr(value: u8, count: u8) -> Result[u8, ShiftCountError[u8]]:
    Ok(value >> count)
def main() -> Result[(), ShiftCountError[i64] | ShiftCountError[u8] | IoError]:
    # Left shifts discard high bits rather than overflow.
    print(shl(3, 63)?, shl(-1, 63)? == i64.MIN, shr(-16, 2)?, ushr(240u8, 4u8)?)?
    Ok(())
"#
        ),
        "-9223372036854775808 True -4 15\n"
    );
}

#[test]
fn function_defaults_require_the_error_in_the_contract() {
    for (body, error) in [
        ("a << b", "ShiftCountError[i64]"),
        ("a >> b", "ShiftCountError[i64]"),
    ] {
        let source = format!(
            "def bad(a: i64, b: i64) -> i64:\n    {body}\ndef main() -> ():\n    drop(bad(1, 2))\n"
        );
        let diagnostic = check_diagnostic(&source);
        assert!(diagnostic.contains(error), "{diagnostic}");
        assert!(diagnostic.contains("checked:"), "{diagnostic}");
        assert!(diagnostic.contains("unchecked:"), "{diagnostic}");
    }
    let diagnostic = check_diagnostic(
        "def bad(a: u8, b: u8) -> Result[u8, OverflowError[u8]]:\n    Ok(a << b)\ndef main() -> ():\n    drop(bad(1u8, 2u8))\n",
    );
    assert!(diagnostic.contains("ShiftCountError[u8]"), "{diagnostic}");
}

/// A literal proves the count; a module constant or guard does not.
#[test]
fn literal_counts_add_no_error_and_out_of_range_literals_are_rejected() {
    assert_eq!(
        run_stdout(
            r#"
def literal(a: u8) -> u8:
    (a << 7) | (a >> (0))
def main() -> Result[(), IoError]:
    print(literal(3u8))?
    Ok(())
"#
        ),
        "131\n"
    );
    for (ty, body, message) in [
        ("u8", "a << 8", "shift count 8 is outside 0..8 for u8"),
        ("i8", "a >> -1", "shift count -1 is outside 0..8 for i8"),
    ] {
        let source = format!(
            "def bad(a: {ty}) -> {ty}:\n    {body}\ndef main() -> ():\n    drop(bad({ty}(1)))\n"
        );
        let diagnostic = check_diagnostic(&source);
        assert!(diagnostic.contains(message), "{diagnostic}");
    }
    for body in [
        "const shift: u8 = 3\ndef bad(a: u8) -> u8:\n    a << shift\n",
        "def bad(a: u8, n: u8) -> u8:\n    if n < 8u8:\n        return a << n\n    0u8\n",
    ] {
        let diagnostic = check_diagnostic(&format!("{body}def main() -> ():\n    pass\n"));
        assert!(diagnostic.contains("ShiftCountError[u8]"), "{diagnostic}");
    }
}

#[test]
fn generic_and_compound_shifts_name_the_instantiated_type() {
    assert_eq!(
        run_stdout(
            r#"
def shl[T: IntType](value: T, count: T) -> Result[T, ShiftCountError[T]]:
    Ok(value << count)
def compound(value: u16, count: u16) -> Result[u16, ShiftCountError[u16]]:
    mut bits = value
    bits <<= count
    bits >>= 1u16
    Ok(bits)
def main() -> Result[(), IoError]:
    print(shl(1u32, 4u32).unwrap(), shl(1i8, 8i8).is_err(), shl(1u64, 64u64).is_err())?
    print(compound(3u16, 2u16).unwrap(), compound(3u16, 16u16).is_err())?
    Ok(())
"#
        ),
        "16 True True\n6 True\n"
    );
}

#[test]
fn local_recovery_and_nested_trapping_suites() {
    assert_eq!(
        run_stdout(
            r#"
def recovered(value: i32, count: i32) -> i32:
    result = checked:
        value << count
    match result:
        case Ok(bits):
            bits
        case Err(ShiftCountError(count)):
            count
def mixed(a: i64, b: i64) -> i64:
    result = checked:
        (a << b) // b + 1
    match result:
        case Ok(value):
            value
        case Err(OverflowError(_)):
            -1
        case Err(ShiftCountError(_)):
            -2
        case Err(ZeroDivisionError()):
            -3
def nested(a: i64, b: i64) -> i64:
    result = checked:
        inner = unchecked:
            a + 1
        inner << b
    match result:
        case Ok(value):
            value
        case Err(_):
            0
def main() -> Result[(), IoError]:
    print(recovered(1, 4), recovered(1, 32), recovered(1, -5))?
    print(mixed(1, 2), mixed(1, 64), mixed(1, 0), mixed(i64.MAX, 1))?
    print(nested(1, 2), nested(1, 70))?
    Ok(())
"#
        ),
        "16 32 -5\n3 -2 -3 -1\n8 0\n"
    );
}

/// `unchecked:` and `unchecked trap:` keep the runtime trap for invalid counts.
#[test]
fn trapping_suites_keep_the_shift_trap() {
    let source = r#"
def main() -> Result[(), AllocError | IoError]:
    choice = args()?[1]
    count = 64 if choice == "0" else 65
    if choice == "0":
        value = unchecked:
            1 << count
        print(value)?
    else:
        result = checked:
            value = unchecked trap:
                1 >> count
            value + 1
        print(result.is_err())?
    Ok(())
"#;
    let message = "shift count outside integer width".to_string();
    assert_trap_cases(source, &[message.clone(), message]);
}

#[test]
fn failures_release_locals_without_writing() {
    assert_eq!(
        run_stdout(
            r#"
class Owner:
    id: i64
    def __del__(self: &mut Owner) -> ():
        print(self.id).unwrap()
def fail(n: &mut i64, count: i64) -> Result[(), AllocError | ShiftCountError[i64]]:
    owner = Owner(10)
    items = [1, 2, 3]?
    *n <<= count
    drop(owner)
    drop(items)
    Ok(())
def main() -> Result[(), IoError]:
    mut n = 1
    print(fail(&mut n, 64).is_err(), n)?
    Ok(())
"#
        ),
        "10\nTrue 1\n"
    );
}

#[test]
fn float_try_from_matches_the_cast_and_reports_unrepresentable_values() {
    assert_eq!(
        run_stdout(
            r#"
def show(result: Result[u8, RangeError]) -> i64:
    match result:
        case Ok(value):
            i64(value)
        case Err(RangeError.OutOfRange):
            -1
def main() -> Result[(), IoError | RangeError]:
    print(show(u8.try_from(255.9)), show(u8.try_from(-0.9)), show(u8.try_from(256.0)), show(u8.try_from(-1.0)))?
    print(show(u8.try_from(f64.NAN)), show(u8.try_from(f64.INF)), show(u8.try_from(-f64.INF)), show(u8.try_from(7.5f32)))?
    print(i64.try_from(-9223372036854775808.0)?, i64.try_from(9223372036854775808.0).is_err())?
    print(i8.try_from(-128.7)?, i8.try_from(-129.0).is_err(), u64.try_from(18446744073709549568.0)?)?
    Ok(())
"#
        ),
        "255 0 -1 -1\n-1 -1 -1 7\n-9223372036854775808 True\n-128 True 18446744073709549568\n"
    );
}

/// Neither a shift error nor its local recovery allocates.
#[cfg(feature = "runtime-checks")]
#[test]
fn shift_errors_and_recovery_need_no_allocation() {
    support::assert_visible_stdout(
        r#"
def shl(value: u64, count: u64) -> Result[u64, ShiftCountError[u64]]:
    Ok(value << count)
def recovered(value: u64, count: u64) -> u64:
    result = checked:
        value >> count
    match result:
        case Ok(bits):
            bits
        case Err(ShiftCountError(count)):
            count
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
failed = shl(1u64, 99u64).is_err()
count = recovered(1u64, 77u64)
converted = u8.try_from(f64.NAN).is_err()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(failed, count, converted).unwrap()
"#,
        "True 77 True",
    );
}

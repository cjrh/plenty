//! BigInt is implemented in Plenty; fixed-width Rust arithmetic is an independent oracle.
use std::process::Command;

fn run(body: &str, optimize: bool) -> String {
    let source = format!(
        "from std.bigint import BigInt, BigIntError, from_i64, from_u64, parse\nfrom std.fmt import FormatError\ndef main() -> Result[(), AllocError | BigIntError | IoError | FormatError]:\n{body}    Ok(())\n"
    );
    run_program(&source, optimize)
}

fn run_program(source: &str, optimize: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let exe = dir.path().join("program");
    plenty::compile_file_to_executable_with_options(
        &path,
        &exe,
        None,
        &plenty::CompileOptions {
            optimize,
            ..Default::default()
        },
    )
    .unwrap();
    let output = Command::new(exe).output().unwrap();
    assert!(
        output.status.success(),
        "status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn arithmetic_matches_fixed_width_oracle() {
    let mut body = String::new();
    let mut expected = String::new();
    let mut state = 0x123456789abcdef0u64;
    for i in 0..32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = (i128::from(state) << 58) | i128::from(state.rotate_left(19));
        let a = if i % 2 == 0 { a } else { -a };
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b = i128::from(state | 1);
        let b = if i % 3 == 0 { -b } else { b };
        body.push_str(&format!("    a{i} = parse(\"{a}\")?\n    b{i} = parse(\"{b}\")?\n    print((a{i} + b{i})?, (a{i} - b{i})?, (a{i} // b{i})?, (a{i} % b{i})?, (a{i} & b{i})?, (a{i} | b{i})?, (a{i} ^ b{i})?, (a{i} >> 67)?)?\n"));
        let mut q = a / b;
        let mut r = a % b;
        if r != 0 && ((a < 0) != (b < 0)) {
            q -= 1;
            r += b;
        }
        expected.push_str(&format!(
            "{} {} {q} {r} {} {} {} {}\n",
            a + b,
            a - b,
            a & b,
            a | b,
            a ^ b,
            a >> 67
        ));
    }
    assert_eq!(run(&body, false), expected);
    assert_eq!(run(&body, true), expected);
}

#[test]
fn boundaries_canonicalization_and_limits() {
    let body = r#"    a = from_i64(i64.MIN)
    b = (-a)?
    c = (b - from_i64(1))?
    print(a, b, c, str.repr(b.to_i64())?, from_u64(u64.MAX)?.to_u64()?)?
    print(c == from_i64(i64.MAX), (b + a)? == from_i64(0), parse("-000")? == from_i64(0))?
    print(str.repr(parse(""))?, str.repr(parse("+"))?, str.repr(parse(" 1"))?, str.repr(parse("0x10", 16))?, str.repr(parse("2", 2))?, str.repr(parse("1", 1))?)?
    print(str.repr(parse("123", max_digits=2))?, str.repr(parse("0", max_digits=0))?, str.repr(from_i64(0).to_string(max_digits=0))?, str.repr(from_i64(99).to_string(max_digits=1))?)?
    print(parse("Z", 36)?.to_string(36)?, str.repr(from_i64(-1).checked_divmod(&from_i64(0)))?)?
    print((from_i64(-1) >> 999)?, (from_i64(1) >> 999)?, (from_i64(0) << i64.MAX)?)?
    print((a // from_i64(-1))? == b, (a % from_i64(-1))? == from_i64(0), b.bit_length(), a.bit_length())?
"#;
    let expected = "-9223372036854775808 9223372036854775808 9223372036854775807 Result[i64, BigIntError].Err(BigIntError.OutOfRange) 18446744073709551615\nTrue True True\nResult[BigInt, BigIntError].Err(BigIntError.InvalidDigit) Result[BigInt, BigIntError].Err(BigIntError.InvalidDigit) Result[BigInt, BigIntError].Err(BigIntError.InvalidDigit) Result[BigInt, BigIntError].Err(BigIntError.InvalidDigit) Result[BigInt, BigIntError].Err(BigIntError.InvalidDigit) Result[BigInt, BigIntError].Err(BigIntError.InvalidRadix)\nResult[BigInt, BigIntError].Err(BigIntError.LimitExceeded) Result[BigInt, BigIntError].Err(BigIntError.LimitExceeded) Result[str, BigIntError].Err(BigIntError.LimitExceeded) Result[str, BigIntError].Err(BigIntError.LimitExceeded)\nz Result[(BigInt, BigInt), BigIntError].Err(BigIntError.ZeroDivision)\n-1 0 0\nTrue True 64 64\n";
    let expected = expected
        .replace("(BigInt, BigInt)", "tuple[BigInt, BigInt]")
        .replace("BigInt", "std.bigint.BigInt");
    assert_eq!(run(body, false), expected);
}

#[test]
fn limb_primitives_match_u128() {
    let mut body = String::new();
    let mut expected = String::new();
    for (a, b, carry) in [
        (0u64, 0u64, false),
        (u64::MAX, u64::MAX, true),
        (1, u64::MAX, false),
        (1, u64::MAX, true),
        (0xdeadbeef01234567, 0xfedcba9876543210, true),
    ] {
        body.push_str(&format!(
            "    print({a}u64.widening_mul({b}u64), {a}u64.carrying_add({b}u64, {}))?\n",
            if carry { "True" } else { "False" }
        ));
        let product = u128::from(a) * u128::from(b);
        let sum = u128::from(a) + u128::from(b) + u128::from(carry);
        expected.push_str(&format!(
            "({}, {}) ({}, {})\n",
            product as u64,
            product >> 64,
            sum as u64,
            if sum >> 64 != 0 { "True" } else { "False" }
        ));
    }
    assert_eq!(run(&body, false), expected);
    assert_eq!(run(&body, true), expected);
}

#[test]
fn every_radix_round_trips_and_float_rounding_is_exact() {
    let n = u128::MAX;
    let mut body = String::new();
    let mut expected = String::new();
    for radix in 2..=36u128 {
        let mut value = n;
        let mut digits = Vec::new();
        while value > 0 {
            digits.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(value % radix) as usize] as char);
            value /= radix;
        }
        let text: String = digits.into_iter().rev().collect();
        body.push_str(&format!(
            "    print(parse(\"{text}\", {radix})?.to_string({radix})?)?\n"
        ));
        expected.push_str(&format!("{text}\n"));
    }
    for n in [
        9007199254740991i128,
        9007199254740993,
        9007199254740995,
        (1i128 << 120) + (1i128 << 66) + 1,
        -((1i128 << 120) + (1i128 << 66) + 1),
        i128::MAX,
        i128::MIN,
    ] {
        body.push_str(&format!(
            "    print(parse(\"{n}\")?.to_f64() == {:.17e})?\n",
            n as f64
        ));
        expected.push_str("True\n");
    }
    body.push_str("    print((from_i64(1) << 1024)?.to_f64() == f64.INF, (from_i64(-1) << 1024)?.to_f64() == -f64.INF)?\n");
    expected.push_str("True True\n");
    assert_eq!(run(&body, false), expected);
    assert_eq!(run(&body, true), expected);
}

#[test]
fn large_products_and_division_cross_algorithm_boundaries() {
    let mut body = String::new();
    let mut expected = String::new();
    for limbs in [31, 32, 33, 64] {
        let n = limbs * 16;
        let input = "f".repeat(n);
        // (16^n - 1)^2 = (16^n - 2) * 16^n + 1.
        let square = format!("{}e{}1", "f".repeat(n - 1), "0".repeat(n - 1));
        body.push_str(&format!("    a{limbs} = parse(\"{input}\", 16)?\n    square{limbs} = (a{limbs} * a{limbs})?\n    print(square{limbs}.to_string(16)? == \"{square}\", (square{limbs} // a{limbs})? == a{limbs}, (square{limbs} % a{limbs})? == from_i64(0))?\n"));
        expected.push_str("True True True\n");
    }
    // Deliberately unbalanced schoolbook multiplication plus a normalized
    // multi-digit divisor whose leading base-2^32 digit is small.
    body.push_str("    a = (from_i64(1) << 4096)?\n    b = parse(\"18446744073709551617\")?\n    product = (a * b)?\n    print((product // b)? == a, (product % b)? == from_i64(0))?\n");
    expected.push_str("True True\n");
    assert_eq!(run(&body, false), expected);
    assert_eq!(run(&body, true), expected);
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_propagate_and_inline_arithmetic_does_not_allocate() {
    let output = run(
        r#"    large = parse("18446744073709551616")?
    mut destination = parse("18446744073709551616")?
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    a = from_i64(41)
    b = from_i64(-7)
    sum = (a + b)?
    difference = (a - b)?
    product = (a * b)?
    quotient = (a // b)?
    remainder = (a % b)?
    shifted = (a << 2)?
    right = (b >> 5)?
    bits = (a ^ b)?
    print("__test_end_no_allocations__")?
    addition = large + from_i64(1)
    multiplication = large * large
    division = large.checked_divmod(&from_i64(3))
    parsing = parse("18446744073709551616")
    rendering = large.to_string()
    assignment = destination.add_assign(&large)
    print("__test_restore_allocations__")?
    print(sum, difference, product, quotient, remainder, shifted, right, bits)?
    print(addition.is_err(), multiplication.is_err(), division.is_err(), parsing.is_err(), rendering.is_err(), assignment.is_err(), destination == large)?
"#,
        false,
    );
    assert!(
        output.ends_with("34 48 -287 -6 -1 164 -1 -48\nTrue True True True True True True\n"),
        "{output}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn partial_allocation_failures_leave_operands_and_assignment_intact() {
    let output = run(
        r#"    seed = parse("12345678901234567890123456789012345678901234567890")?
    for budget in range(20):
        mut destination = copy(seed)?
        marker = f"__test_fail_allocations_after_{budget}__"?
        print(marker)?
        result = destination.add_assign(&seed)
        product = seed * seed
        parsed = parse("12345678901234567890123456789012345678901234567890")
        rendered = seed.to_string()
        print("__test_restore_allocations__")?
        if result.is_err():
            if destination != seed:
                return Err(BigIntError.OutOfRange)
        drop(product)
        drop(parsed)
        drop(rendered)
    print("preserved")?
"#,
        false,
    );
    assert!(output.ends_with("preserved\n"), "{output}");
}

#[test]
fn formatting_hooks_respect_the_active_sum_payload() {
    let source = r#"from std.fmt import Formatter, FormatError
class Pair:
    a: i64
    b: i64
enum Value:
    Small(i64)
    Large(Pair, list[u64])
    def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        match self:
            case Value.Small(n):
                out.text("small:")?
                out.text(str.repr(*n).map_error(FormatError.Allocation)?)
            case Value.Large(pair, words):
                out.text("large:")?
                out.text(str.repr(pair.a).map_error(FormatError.Allocation)?)?
                out.text(str.repr(words[0]).map_error(FormatError.Allocation)?)
class Number:
    value: Value
    def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
        self.value.debug(out)
def main() -> Result[(), AllocError | IoError | FormatError]:
    small = Number(Value.Small(7))
    large = Number(Value.Large(Pair(3, 4), [9u64]?))
    print(small, large)?
    values = [Value.Small(8), Value.Large(Pair(5, 6), [2u64]?)]?
    print(values)?
    print(small, large)?
    Ok(())
"#;
    for optimize in [false, true] {
        assert_eq!(
            run_program(source, optimize),
            "small:7 large:39\n[small:8, large:52]\nsmall:7 large:39\n"
        );
    }
}

#[test]
fn zero_divisors_and_negative_shifts_trap_at_the_operation() {
    crate::test_support::assert_trap_cases(
        r#"from std.bigint import from_i64
def main() -> Result[(), AllocError | IoError | ParseError]:
    argv = args()?
    index = i64.parse(argv[1])?
    if index == 0:
        drop(from_i64(1) // from_i64(0))
    elif index == 1:
        drop(from_i64(1) % from_i64(0))
    elif index == 2:
        drop(from_i64(1) << -1)
    else:
        drop(from_i64(1) >> -1)
    Ok(())
"#,
        &[
            "BigInt.__floordiv__: divisor is zero".into(),
            "BigInt.__mod__: divisor is zero".into(),
            "BigInt.__lshift__: count is negative".into(),
            "BigInt.__rshift__: count is negative".into(),
        ],
    );
}

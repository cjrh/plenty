//! The bare numeric cast `T(x)` traps on a value `T` cannot represent, and
//! `T.wrapping_from(x)` keeps an integer's low bits.
mod support;
use rstest::rstest;
use std::process::{Command, Output};

const TRAP: &str = ": integer cast out of range\n";

/// A trap report without its `line:column` position, which
/// `test_trap_locations` covers.
fn message(output: &Output) -> String {
    let error = String::from_utf8_lossy(&output.stderr);
    let Some((position, message)) = error
        .strip_prefix("error: ")
        .and_then(|rest| rest.split_at_checked(rest.find(": ")?))
    else {
        return error.into_owned();
    };
    assert!(
        position.split(':').all(|n| n.parse::<u32>().is_ok()),
        "{error}"
    );
    message.to_owned()
}

const INTEGERS: [(&str, i128, i128); 8] = [
    ("i8", i8::MIN as i128, i8::MAX as i128),
    ("i16", i16::MIN as i128, i16::MAX as i128),
    ("i32", i32::MIN as i128, i32::MAX as i128),
    ("i64", i64::MIN as i128, i64::MAX as i128),
    ("u8", 0, u8::MAX as i128),
    ("u16", 0, u16::MAX as i128),
    ("u32", 0, u32::MAX as i128),
    ("u64", 0, u64::MAX as i128),
];

fn assert_output(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

fn assert_trap(output: &Output, context: &str) {
    assert_eq!(output.status.code(), Some(1), "{context}");
    assert_eq!(message(output), TRAP, "{context}");
    assert!(output.stdout.is_empty(), "{context}");
}

fn assert_rejected(source: &str, expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{source}\n{error}");
}

/// One executable holding every case: its argument selects the case to run,
/// because a trap ends the process. Case 0 runs every conversion that
/// succeeds; each later case runs one conversion that must trap.
struct Cases {
    declarations: String,
    succeeding: String,
    expected: String,
    trapping: Vec<String>,
}

impl Cases {
    fn new(declarations: String) -> Self {
        Self {
            declarations,
            succeeding: String::new(),
            expected: String::new(),
            trapping: Vec::new(),
        }
    }

    fn succeeds(&mut self, call: &str, expected: &str) {
        self.succeeding
            .push_str(&format!("        print({call}).unwrap()\n"));
        self.expected.push_str(&format!("{expected}\n"));
    }

    fn traps(&mut self, call: String) {
        self.trapping.push(call);
    }

    fn check(self) {
        let mut source = format!(
            "{}def main() -> ():\n    arguments = args().unwrap()\n    chosen = i64.parse(&arguments[1]).unwrap()\n    if chosen == 0:\n{}",
            self.declarations, self.succeeding
        );
        for (index, call) in self.trapping.iter().enumerate() {
            source.push_str(&format!(
                "    if chosen == {}:\n        print({call}).unwrap()\n",
                index + 1
            ));
        }
        let workspace = tempfile::tempdir().unwrap();
        let executable = workspace.path().join("program");
        support::compile_source_to_executable(&source, &executable)
            .unwrap_or_else(|error| panic!("{source}\n{error}"));
        let run = |case: usize| {
            Command::new(&executable)
                .arg(case.to_string())
                .output()
                .unwrap()
        };
        let output = run(0);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), self.expected);
        assert!(!self.trapping.is_empty());
        for (index, call) in self.trapping.iter().enumerate() {
            assert_trap(&run(index + 1), call);
        }
    }
}

/// The reproduction from issue #65. Values arrive through variables, so the
/// checks run in the compiled program.
#[rstest]
#[case("x = 300\nprint(u8(x)).unwrap()")]
#[case("y = -1\nprint(u64(y)).unwrap()")]
#[case("z = 200u8\nprint(i8(z)).unwrap()")]
#[case("f = 300.7\nprint(u8(f)).unwrap()")]
#[case("type Byte = u8\nx = 257\nprint(Byte(x)).unwrap()")]
#[case("def narrow[T: IntType](value: T) -> u8:\n    u8(value)\nprint(narrow(256u16)).unwrap()")]
fn out_of_range_casts_trap(#[case] source: &str) {
    assert_trap(&support::run(source), source);
}

#[test]
fn a_trap_follows_output_already_printed() {
    let output = support::run("print('before').unwrap()\nx = 300\nprint(u8(x)).unwrap()");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "before\n");
    assert_eq!(message(&output), TRAP);
}

/// Every source/target pair at both ends of both ranges and one step outside
/// the target's, so each bound is tried from the side that fits and the side
/// that does not. Values arrive through a parameter so the check runs on a
/// register of the source width, not on a literal.
#[test]
fn integer_casts_check_every_width_and_signedness() {
    let mut declarations = String::new();
    for (from, _, _) in INTEGERS {
        for (to, _, _) in INTEGERS {
            declarations.push_str(&format!(
                "def {from}_to_{to}(value: {from}) -> {to}:\n    {to}(value)\n"
            ));
        }
    }
    let mut cases = Cases::new(declarations);
    for (from, from_min, from_max) in INTEGERS {
        for (to, to_min, to_max) in INTEGERS {
            let mut values = vec![
                from_min,
                from_max,
                -1,
                0,
                to_min - 1,
                to_min,
                to_max,
                to_max + 1,
            ];
            values.retain(|value| (from_min..=from_max).contains(value));
            values.sort_unstable();
            values.dedup();
            for value in values {
                let call = format!("{from}_to_{to}({value}{from})");
                if (to_min..=to_max).contains(&value) {
                    cases.succeeds(&call, &value.to_string());
                } else {
                    cases.traps(call);
                }
            }
        }
    }
    cases.check();
}

/// The floats nearest each bound of each integer type, from both sides: a
/// float converts exactly when its truncation toward zero is in range.
#[test]
fn float_casts_truncate_then_check_the_range() {
    let mut declarations = String::from(
        "def nan_f32() -> f32:\n    zero = 0f32\n    zero / zero\ndef inf_f32() -> f32:\n    zero = 0f32\n    1f32 / zero\ndef nan_f64() -> f64:\n    zero = 0.0\n    zero / zero\ndef inf_f64() -> f64:\n    zero = 0.0\n    1.0 / zero\n",
    );
    for float in ["f32", "f64"] {
        for (to, _, _) in INTEGERS {
            declarations.push_str(&format!(
                "def {float}_to_{to}(value: {float}) -> {to}:\n    {to}(value)\n"
            ));
        }
    }
    let mut cases = Cases::new(declarations);
    for (to, to_min, to_max) in INTEGERS {
        // `to_max + 1` is a power of two, so it is exact in both float types.
        let end = (to_max + 1) as f64;
        let below = (to_min - 1) as f64;
        let (low_out, low_in) = if below as i128 == to_min - 1 {
            (below, below.next_up())
        } else {
            (below.next_down(), below)
        };
        for value in [low_in, -0.9, 0.0, 12.75, end.next_down()] {
            cases.succeeds(
                &format!("f64_to_{to}({value:?})"),
                &(value.trunc() as i128).to_string(),
            );
        }
        for value in [low_out, end] {
            cases.traps(format!("f64_to_{to}({value:?})"));
        }

        let end = (to_max + 1) as f32;
        let below = (to_min - 1) as f32;
        let (low_out, low_in) = if below as i128 == to_min - 1 {
            (below, below.next_up())
        } else {
            (below.next_down(), below)
        };
        for value in [low_in, -0.9, 0.0, 12.75, end.next_down()] {
            cases.succeeds(
                &format!("f32_to_{to}({value:?}f32)"),
                &(value.trunc() as i128).to_string(),
            );
        }
        for value in [low_out, end] {
            cases.traps(format!("f32_to_{to}({value:?}f32)"));
        }

        for float in ["f32", "f64"] {
            cases.traps(format!("{float}_to_{to}(nan_{float}())"));
            cases.traps(format!("{float}_to_{to}(inf_{float}())"));
            cases.traps(format!("{float}_to_{to}(-inf_{float}())"));
        }
    }
    cases.check();
}

#[rstest]
#[case(
    "print(i64(7u8)).unwrap()\nprint(u64(7i8)).unwrap()\nprint(u8(255)).unwrap()\nprint(i8(-128)).unwrap()\nprint(i8(127u64)).unwrap()\nprint(u64(9223372036854775807)).unwrap()\nprint(i64(9223372036854775807u64)).unwrap()",
    "7\n7\n255\n-128\n127\n9223372036854775807\n9223372036854775807\n"
)]
#[case(
    "print(u8(255.9)).unwrap()\nprint(u8(-0.9)).unwrap()\nprint(i8(-128.9)).unwrap()\nprint(i32(-2.75f32)).unwrap()\nprint(i64(-9223372036854775808.0)).unwrap()\nprint(f32(300)).unwrap()\nprint(f64(1.5f32)).unwrap()\nprint(f32(1e300)).unwrap()",
    "255\n0\n-128\n-2\n-9223372036854775808\n300.0\n1.5\ninf\n"
)]
fn in_range_literal_casts_convert(#[case] source: &str, #[case] expected: &str) {
    assert_output(source, expected);
}

/// Only a literal operand is rejected at compile time: a literal on a path
/// the program does not take must not stop it compiling.
#[test]
fn a_literal_inside_a_larger_operand_is_checked_when_it_is_reached() {
    let pick = "def pick(flag: bool) -> u8:\n    u8(1 if flag else 300)\ndef pick_float(flag: bool) -> u8:\n    u8(1.5 if flag else 300.5)\n";
    assert_output(
        &format!("{pick}print(pick(True)).unwrap()\nprint(pick_float(True)).unwrap()"),
        "1\n1\n",
    );
    for call in ["pick(False)", "pick_float(False)"] {
        let source = format!("{pick}print({call}).unwrap()");
        assert_trap(&support::run(&source), &source);
    }
}

/// A literal operand is checked when the program is compiled.
#[rstest]
#[case(
    "x = u8(300)",
    "300 is out of range for u8; u8.wrapping_from keeps the low bits"
)]
#[case("x = u8(256)", "256 is out of range for u8")]
#[case("x = u8(-1)", "-1 is out of range for u8")]
#[case("x = i8(-129)", "-129 is out of range for i8")]
#[case("x = i8(200u8)", "200 is out of range for i8")]
#[case("x = u64(-1)", "-1 is out of range for u64")]
#[case(
    "x = i64(18446744073709551615u64)",
    "18446744073709551615 is out of range for i64"
)]
#[case("type Byte = u8\nx = Byte(257)", "257 is out of range for u8")]
#[case("x = u8(300.7)", "300.7 is out of range for u8")]
#[case("x = u8(256.0)", "256.0 is out of range for u8")]
#[case("x = u8(-1.0)", "-1.0 is out of range for u8")]
#[case("x = i8(-129.0f32)", "-129.0 is out of range for i8")]
#[case("x = i64(1e100)", "1e100 is out of range for i64")]
#[case("x = i64(9223372036854775808.0)", "is out of range for i64")]
fn out_of_range_literal_casts_are_rejected(#[case] source: &str, #[case] expected: &str) {
    assert_rejected(source, expected);
}

#[rstest]
#[case(
    "print(u8.wrapping_from(300)).unwrap()\nprint(u64.wrapping_from(-1)).unwrap()\nprint(i8.wrapping_from(200u8)).unwrap()",
    "44\n18446744073709551615\n-56\n"
)]
#[case(
    "x = 300\ny = -1\nz = 200u8\nprint(u8.wrapping_from(x)).unwrap()\nprint(u64.wrapping_from(y)).unwrap()\nprint(i8.wrapping_from(z)).unwrap()",
    "44\n18446744073709551615\n-56\n"
)]
#[case(
    "type Byte = u8\nx = 257\nprint(Byte.wrapping_from(x)).unwrap()\nprint(u8.wrapping_from(x) + 1u8).unwrap()",
    "1\n2\n"
)]
fn wrapping_conversions_keep_the_low_bits(#[case] source: &str, #[case] expected: &str) {
    assert_output(source, expected);
}

#[test]
fn wrapping_conversions_cover_every_width_and_signedness() {
    let mut source = String::new();
    let mut expected = String::new();
    for (from, from_min, from_max) in INTEGERS {
        for (to, _, _) in INTEGERS {
            source.push_str(&format!(
                "def {from}_to_{to}(value: {from}) -> {to}:\n    {to}.wrapping_from(value)\n"
            ));
        }
        for (to, to_min, to_max) in INTEGERS {
            let mut values = vec![
                from_min,
                from_max,
                -1,
                0,
                to_min - 1,
                to_min,
                to_max,
                to_max + 1,
            ];
            values.retain(|value| (from_min..=from_max).contains(value));
            values.sort_unstable();
            values.dedup();
            let modulus = to_max - to_min + 1;
            for value in values {
                let wrapped = (value - to_min).rem_euclid(modulus) + to_min;
                source.push_str(&format!("print({from}_to_{to}({value}{from})).unwrap()\n"));
                expected.push_str(&format!("{wrapped}\n"));
            }
        }
    }
    assert_output(&source, &expected);
}

#[rstest]
#[case("x = u8.wrapping_from()", "wrapping_from takes one integer argument")]
#[case(
    "x = u8.wrapping_from(1, 2)",
    "wrapping_from takes one integer argument"
)]
#[case("x = u8.wrapping_from(1.5)", "wrapping_from requires an integer value")]
#[case("x = u8.wrapping_from('1')", "wrapping_from requires an integer value")]
#[case(
    "x = u8.wrapping_from(True)",
    "wrapping_from requires an integer value"
)]
#[case(
    "x = f64.wrapping_from(1)",
    "wrapping_from is defined for integer types; use a cast for floats"
)]
#[case(
    "x = str.wrapping_from(1)",
    "wrapping_from is defined for integer types"
)]
#[case(
    "x = bool.wrapping_from(1)",
    "wrapping_from is defined for integer types"
)]
#[case(
    "x = list[u8].wrapping_from(1)",
    "wrapping_from is defined for integer types"
)]
#[case("x: i8 = u8.wrapping_from(1)", "expected i8, got u8")]
fn wrapping_conversions_reject_other_types(#[case] source: &str, #[case] expected: &str) {
    assert_rejected(source, expected);
}

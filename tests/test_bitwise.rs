mod support;

#[test]
fn integer_bits_and_wrapping_work_at_every_width() {
    for ty in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        let output = support::run(&format!(
            "a = 5{ty}\nb = 3{ty}\nprint(a & b).unwrap()\nprint(a | b).unwrap()\nprint(a ^ b).unwrap()\nprint(a << 1).unwrap()\nprint(a >> 1).unwrap()\nprint((~a) ^ a).unwrap()\nprint(a.wrapping_add(b)).unwrap()\nprint(a.wrapping_sub(b)).unwrap()\nprint(a.wrapping_mul(b)).unwrap()\n"
        ));
        assert!(output.status.success());
        let all_bits = if ty.starts_with('i') {
            "-1".into()
        } else {
            match ty {
                "u8" => u8::MAX.to_string(),
                "u16" => u16::MAX.to_string(),
                "u32" => u32::MAX.to_string(),
                _ => u64::MAX.to_string(),
            }
        };
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("1\n7\n6\n10\n2\n{all_bits}\n8\n2\n15\n")
        );
    }
}

#[test]
fn precedence_borrows_and_overflow_boundaries() {
    let output = support::run(
        "x = 12\nprint(*(&x) & 5).unwrap()\nprint(1 | 2 ^ 3 & 4 << 1 + 1).unwrap()\nprint(-8 >> 2).unwrap()\nprint(128u8 >> 7).unwrap()\nprint(1u8 << 7).unwrap()\nprint(127i8.wrapping_add(1)).unwrap()\nprint(0u8.wrapping_sub(1)).unwrap()\nprint(64u8.wrapping_mul(4)).unwrap()\nprint(18446744073709551615u64.wrapping_mul(6364136223846793005)).unwrap()\n"
    );
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "4\n3\n-2\n1\n128\n-128\n255\n0\n12082607849862758611\n"
    );
}

#[test]
fn invalid_counts_trap_before_machine_shift_masking() {
    for (ty, width) in [
        ("i8", 8),
        ("u8", 8),
        ("i16", 16),
        ("u16", 16),
        ("i32", 32),
        ("u32", 32),
        ("i64", 64),
        ("u64", 64),
    ] {
        for op in ["<<", ">>"] {
            let output = support::run(&format!("print(1{ty} {op} {width}).unwrap()\n"));
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr)
                .contains("shift count outside integer width"));
        }
    }
    let output = support::run("print(1 << -1).unwrap()\n");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("shift count outside integer width"));
}

#[test]
fn reject_noninteger_and_mixed_width_operations() {
    for source in [
        "print(1.0 & 2.0).unwrap()",
        "print(~1.0).unwrap()",
        "print(True | False).unwrap()",
        "print(1u8 << 1u16).unwrap()",
        "print(1.0.wrapping_add(2.0)).unwrap()",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

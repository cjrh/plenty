mod support;

#[test]
fn unsigned_minus_is_rejected_for_literals_in_every_context() {
    for ty in ["u8", "u16", "u32", "u64"] {
        for literal in ["0", "1", "(0)", "((1))"] {
            for source in [
                format!("value: {ty} = -{literal}"),
                format!("def value() -> {ty}:\n    -{literal}\nprint(value()).unwrap()"),
                format!("def consume(value: &{ty}) -> ():\n    pass\nconsume(-{literal})"),
                format!("values: list[{ty}] = [-{literal}].unwrap()"),
            ] {
                let error = support::check_source(&source).unwrap_err().to_string();
                assert!(
                    error.contains(&format!(
                        "unary minus is not defined for unsigned type {ty}"
                    )),
                    "{source}\n{error}"
                );
            }
        }
        for source in [format!("value = -0{ty}"), format!("value = -1{ty}")] {
            let error = support::check_source(&source).unwrap_err().to_string();
            assert!(error.contains(&format!("unsigned type {ty}")), "{error}");
        }
    }
}

#[test]
fn signed_minima_and_float_negative_zero_are_preserved() {
    let output = support::run(
        "print(-128i8).unwrap()\nprint(-32768i16).unwrap()\nprint(-2147483648i32).unwrap()\nprint(-9223372036854775808i64).unwrap()\nprint(-0.0).unwrap()\nprint(-0f32).unwrap()\nprint(0u64).unwrap()",
    );
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"-128\n-32768\n-2147483648\n-9223372036854775808\n-0.0\n-0.0\n0\n"
    );
}

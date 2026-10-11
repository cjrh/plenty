mod support;

#[test]
fn conversion_error_match_is_exhaustive_with_one_variant() {
    let output = support::run(
        r#"
def narrow(value: i64) -> Result[u8, RangeError]:
    Ok(u8.try_from(value)?)
for value in [7, 300, -1].unwrap():
    match narrow(value):
        case Ok(number):
            print(number).unwrap()
        case Err(error):
            match error:
                case RangeError.OutOfRange:
                    print("out of range").unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"7\nout of range\nout of range\n");
}

#[test]
fn conversion_does_not_implicitly_convert_to_parse_error() {
    let error = support::check_source(
        "def narrow(value: i64) -> Result[u8, ParseError]:\n    Ok(u8.try_from(value)?)",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("identical Result error types"), "{error}");
    let error = support::check_source("error = RangeError.Invalid")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Invalid"), "{error}");
}

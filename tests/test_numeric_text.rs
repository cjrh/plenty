mod support;

fn native(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let visible = text
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected);
}

#[test]
fn integer_parsing_is_typed_and_propagates() {
    native(r#"
type Count = u8
def number(text: &str) -> Result[u8, ParseError]:
    Ok(Count.parse(text)?)
text = " 255 "
print(number(&text))
print(i8.parse("-128"))
print(u64.parse("18446744073709551615"))
print(Count.parse("256"))
print(i64.parse("12x"))
print(i16.parse("+42"))
print(i32.parse("-42"))
print(u16.parse("65535"))
print(u32.parse("4294967295"))
"#, "Result[u8, ParseError].Ok(255)\nResult[i8, ParseError].Ok(-128)\nResult[u64, ParseError].Ok(18446744073709551615)\nResult[u8, ParseError].Err(ParseError.OutOfRange)\nResult[i64, ParseError].Err(ParseError.Invalid)\nResult[i16, ParseError].Ok(42)\nResult[i32, ParseError].Ok(-42)\nResult[u16, ParseError].Ok(65535)\nResult[u32, ParseError].Ok(4294967295)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn parsing_and_errors_allocate_nothing() {
    native(r#"
print("__test_fail_allocations_after_0__")
a = i64.parse("42")
b = u8.parse("999")
c = i32.parse("bad")
print("__test_restore_allocations__")
print(a)
print(b)
print(c)
"#, "Result[i64, ParseError].Ok(42)\nResult[u8, ParseError].Err(ParseError.OutOfRange)\nResult[i32, ParseError].Err(ParseError.Invalid)");
}

#[test]
fn parsing_checks_arity_and_type() {
    for source in ["i64.parse()", "i64.parse(1)", "i64.parse(\"1\", \"2\")"] {
        assert!(support::check_source(source).is_err());
    }
}

#[test]
fn float_parsing_preserves_width_and_reports_overflow() {
    native(r#"
print(f32.parse(" .125 "))
print(f64.parse("-0.0"))
print(f32.parse("1e100"))
print(f64.parse("1e-1000"))
print(f64.parse("inf"))
print(f32.parse("bad"))
"#, "Result[f32, ParseError].Ok(0.125)\nResult[f64, ParseError].Ok(-0.0)\nResult[f32, ParseError].Err(ParseError.OutOfRange)\nResult[f64, ParseError].Ok(0.0)\nResult[f64, ParseError].Ok(inf)\nResult[f32, ParseError].Err(ParseError.Invalid)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn float_parsing_is_allocation_free() {
    native(r#"
print("__test_fail_allocations_after_0__")
a = f64.parse("3.14159265358979323846264338327950288419716939937510")
b = f32.parse("1e100")
c = f64.parse("bad")
print("__test_restore_allocations__")
print(a)
print(b)
print(c)
"#, "Result[f64, ParseError].Ok(3.141592653589793)\nResult[f32, ParseError].Err(ParseError.OutOfRange)\nResult[f64, ParseError].Err(ParseError.Invalid)");
}

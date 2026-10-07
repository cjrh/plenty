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
print(number(&text)).unwrap()
print(i8.parse("-128")).unwrap()
print(u64.parse("18446744073709551615")).unwrap()
print(Count.parse("256")).unwrap()
print(i64.parse("12x")).unwrap()
print(i16.parse("+42")).unwrap()
print(i32.parse("-42")).unwrap()
print(u16.parse("65535")).unwrap()
print(u32.parse("4294967295")).unwrap()
"#, "Result[u8, ParseError].Ok(255)\nResult[i8, ParseError].Ok(-128)\nResult[u64, ParseError].Ok(18446744073709551615)\nResult[u8, ParseError].Err(ParseError.OutOfRange)\nResult[i64, ParseError].Err(ParseError.Invalid)\nResult[i16, ParseError].Ok(42)\nResult[i32, ParseError].Ok(-42)\nResult[u16, ParseError].Ok(65535)\nResult[u32, ParseError].Ok(4294967295)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn parsing_and_errors_allocate_nothing() {
    native(r#"
print("__test_fail_allocations_after_0__").unwrap()
a = i64.parse("42")
b = u8.parse("999")
c = i32.parse("bad")
print("__test_restore_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
"#, "Result[i64, ParseError].Ok(42)\nResult[u8, ParseError].Err(ParseError.OutOfRange)\nResult[i32, ParseError].Err(ParseError.Invalid)");
}

#[test]
fn parsing_checks_arity_and_type() {
    for source in ["i64.parse()", "i64.parse(1)", "i64.parse(\"1\", \"2\")"] {
        assert!(support::check_source(source).is_err());
    }
}

#[test]
fn scalar_formatting_and_round_trip() {
    native(r#"
def render(n: f32) -> Result[str, AllocError]:
    str.from(n)
print(str.from(-128i8)).unwrap()
print(str.from(18446744073709551615u64)).unwrap()
print(render(-0.0f32)).unwrap()
print(str.from(True)).unwrap()
print(str.from(False)).unwrap()
"#, "Result[str, AllocError].Ok(\"-128\")\nResult[str, AllocError].Ok(\"18446744073709551615\")\nResult[str, AllocError].Ok(\"-0.0\")\nResult[str, AllocError].Ok(\"True\")\nResult[str, AllocError].Ok(\"False\")");
    for source in ["str.from()", "str.from([].unwrap())", "str.from(\"x\")"] {
        assert!(support::check_source(source).is_err());
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn scalar_formatting_has_one_recoverable_allocation() {
    for budget in 0..=1 {
        native(
            &format!(
                r#"
print("__test_fail_allocations_after_{budget}__").unwrap()
a = str.from(1.25f64)
print("__test_restore_allocations__").unwrap()
print(a).unwrap()
"#
            ),
            if budget == 0 {
                "Result[str, AllocError].Err(AllocError.OutOfMemory)"
            } else {
                "Result[str, AllocError].Ok(\"1.25\")"
            },
        );
    }
}

#[test]
fn float_parsing_preserves_width_and_reports_overflow() {
    native(r#"
print(f32.parse(" .125 ")).unwrap()
print(f64.parse("-0.0")).unwrap()
print(f32.parse("1e100")).unwrap()
print(f64.parse("1e-1000")).unwrap()
print(f64.parse("inf")).unwrap()
print(f32.parse("bad")).unwrap()
"#, "Result[f32, ParseError].Ok(0.125)\nResult[f64, ParseError].Ok(-0.0)\nResult[f32, ParseError].Err(ParseError.OutOfRange)\nResult[f64, ParseError].Ok(0.0)\nResult[f64, ParseError].Ok(inf)\nResult[f32, ParseError].Err(ParseError.Invalid)");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn float_parsing_is_allocation_free() {
    native(r#"
print("__test_fail_allocations_after_0__").unwrap()
a = f64.parse("3.14159265358979323846264338327950288419716939937510")
b = f32.parse("1e100")
c = f64.parse("bad")
print("__test_restore_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
print(c).unwrap()
"#, "Result[f64, ParseError].Ok(3.141592653589793)\nResult[f32, ParseError].Err(ParseError.OutOfRange)\nResult[f64, ParseError].Err(ParseError.Invalid)");
}

//! Hexadecimal, octal, and binary integer literals (issue #563) and the
//! `\u{H...}` string escape (issue #564), and propagated numeric context (issue #710).
mod support;
use support::check_error as rejected;

#[test]
fn radix_integers_have_decimal_values_suffixes_and_context_types() {
    support::assert_stdout(
        r#"
const FLAGS: u8 = 0b1000_0001
def main() -> Result[(), IoError | OverflowError[i64]]:
    mask: u16 = 0xFF00
    print(0x1F1E6, 0X7f, 0o755, 0b1010, 0x0)?
    print(0xFFu8, -0x80i8, 0xffff_ffff_ffff_ffffu64, -0x8000_0000_0000_0000)?
    print(mask, FLAGS, 0x1E + 1, 0x1f32)?
    match 0x2A:
        case 0x2A:
            print("hex pattern")?
        case _:
            pass
    Ok(())
"#,
        "127462 127 493 10 0\n255 -128 18446744073709551615 -9223372036854775808\n65280 129 31 7986\nhex pattern\n",
    );
}

fn native_output(source: &str, release: bool) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("literal_context.plenty");
    let executable = dir.path().join("literal_context");
    std::fs::write(&path, source).unwrap();
    let mut compiler = std::process::Command::new(support::plenty_bin());
    compiler.env("PLENTY_CACHE", "off");
    if !release {
        compiler.arg("--fast-compile");
    }
    let output = compiler
        .arg("--compile")
        .arg(&path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "release={release}\n{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::process::Command::new(&executable).output().unwrap()
}

/// Exercise native code with both Cranelift optimization modes, not just --check.
fn assert_native_stdout(source: &str, expected: &str) {
    for release in [false, true] {
        let output = native_output(source, release);
        assert!(
            output.status.success(),
            "release={release}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "release={release}"
        );
    }
}

#[test]
fn propagated_results_contextualize_literals_from_known_outputs() {
    assert_native_stdout(
        r#"
type Narrow = Result[u16, ParseError]
def narrow() -> Narrow:
    Ok(8u16)
def byte() -> Result[u8, ParseError]:
    Ok(8u8)
def signed() -> Result[i8, ParseError]:
    Ok(-8i8)
def real() -> Result[f32, ParseError]:
    Ok(2.5f32)
def identity[T](value: T) -> Result[T, ParseError]:
    Ok(value)
class Source:
    def read(self) -> Narrow:
        narrow()
def main() -> Result[(), IoError | ParseError | OverflowError[u8] | OverflowError[u16] | OverflowError[i8]]:
    print(1 + narrow()?, narrow()? + 1)?
    print(1 + byte()?, byte()? + 1)?
    print(-1 + signed()?, signed()? + -1)?
    print(0.5 + real()?, real()? + 0.5)?
    named = narrow
    offset = 8u16
    closure = def [offset]() -> Narrow:
        Ok(offset)
    source = Source()
    print(1 + named()?, named()? + 1)?
    print(1 + closure()?, closure()? + 1)?
    print(1 + source.read()?, source.read()? + 1)?
    print(1 + identity[u16](8u16)?, identity[u16](8u16)? + 1)?
    result: Narrow = narrow()
    print(1 + result?, result? + 1)?
    Ok(())
"#,
        "9 9\n9 9\n-9 -9\n3.0 3.0\n9 9\n9 9\n9 9\n9 9\n9 9\n",
    );
}

#[test]
fn propagated_options_use_the_some_payload_for_numeric_context() {
    assert_native_stdout(
        r#"
type Narrow = Option[u16]
def narrow() -> Narrow:
    Some(8u16)
def left() -> Narrow:
    unchecked:
        value = 1 + narrow()?
        Some(value)
def right() -> Narrow:
    unchecked:
        value = narrow()? + 1
        Some(value)
def float_left() -> Option[f32]:
    value: Option[f32] = Some(2.5f32)
    result = 0.5 + value?
    Some(result)
def main() -> Result[(), IoError]:
    print(left(), right(), float_left())?
    Ok(())
"#,
        "Option[u16].Some(9) Option[u16].Some(9) Option[f32].Some(3.0)\n",
    );
}

#[test]
fn propagated_numeric_hints_preserve_order_cleanup_and_public_errors() {
    assert_native_stdout(
        r#"
class Owner:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def read(fail: bool) -> Result[u16, ParseError]:
    print("read", fail).unwrap()
    if fail:
        return Err(ParseError.Invalid)
    Ok(u16.MAX)
def absent() -> Option[u16]:
    print("absent").unwrap()
    Nothing
def take(owner: Owner, value: u16) -> u16:
    print("take").unwrap()
    value
def failed() -> Result[u16, ParseError | OverflowError[u16]]:
    local = Owner(1)
    Ok(take(Owner(2), 1 + read(True)?))
def missing() -> Option[u16]:
    local = Owner(3)
    unchecked:
        value = take(Owner(4), 1 + absent()?)
        Some(value)
def overflow(left: bool) -> Result[u16, ParseError | OverflowError[u16]]:
    if left:
        return Ok(1 + read(False)?)
    Ok(read(False)? + 1)
def first() -> u16:
    print("first").unwrap()
    1u16
def ordered() -> Result[u16, ParseError | OverflowError[u16]]:
    Ok(first() + read(True)?)
def report(result: Result[u16, ParseError | OverflowError[u16]]) -> ():
    match result:
        case Ok(value):
            print("unexpected", value).unwrap()
        case Err(error):
            print(error).unwrap()
def main() -> Result[(), IoError]:
    report(failed())
    print(missing())?
    report(ordered())
    for left in [True, False].unwrap():
        match overflow(left):
            case Ok(value):
                print("unexpected", value)?
            case Err(error):
                match error:
                    case overflow: OverflowError[u16]:
                        print("overflow", overflow.wrapped)?
                    case parse: ParseError:
                        print("unexpected parse")?
    Ok(())
"#,
        "read True\ndrop 2\ndrop 1\nParseError.Invalid\nabsent\ndrop 4\ndrop 3\nOption[u16].Nothing\nfirst\nread True\nParseError.Invalid\nread False\noverflow 0\nread False\noverflow 0\n",
    );
}

#[test]
fn propagated_numeric_context_preserves_unchecked_overflow_traps() {
    let source = r#"
def narrow() -> Result[u16, ParseError]:
    Ok(u16.MAX)
def main() -> Result[(), ParseError]:
    unchecked:
        drop(1 + narrow()?)
    Ok(())
"#;
    for release in [false, true] {
        let output = native_output(source, release);
        assert_eq!(output.status.code(), Some(1), "release={release}");
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(
            message.contains("integer overflow"),
            "release={release}: {message}"
        );
    }
}

#[test]
fn propagated_numeric_context_preserves_mismatches_ranges_and_contracts() {
    for (expression, error) in [
        ("1i64 + narrow()?", "cannot combine i64 with u16"),
        ("narrow()? + 1i64", "cannot combine u16 with i64"),
        ("65536 + narrow()?", "integer literal out of range for u16"),
        ("narrow()? + 65536", "integer literal out of range for u16"),
    ] {
        let source = format!(
            "def narrow() -> Result[u16, ParseError]:\n    Ok(8u16)\ndef main() -> Result[(), ParseError | OverflowError[u16]]:\n    drop({expression})\n    Ok(())\n"
        );
        let message = rejected(&source);
        assert!(message.contains(error), "{expression}\n{message}");
    }
    let message = rejected(
        "def narrow() -> Result[u16, ParseError]:\n    Ok(8u16)\ndef main() -> Result[(), ParseError]:\n    drop(1 + narrow()?)\n    Ok(())\n",
    );
    assert!(message.contains("OverflowError[u16]"), "{message}");
    let message = rejected(
        "enum NotResult:\n    Ok(u16)\n    Err(ParseError)\ndef narrow() -> NotResult:\n    NotResult.Ok(8u16)\ndef main() -> Result[(), ParseError | OverflowError[u16]]:\n    drop(1 + narrow()?)\n    Ok(())\n",
    );
    assert!(
        message
            .contains("`?` operand and function return must use the same Result or Option family"),
        "{message}"
    );
}

#[test]
fn radix_integers_keep_decimal_range_checks() {
    for (source, error) in [
        ("x = 0x100u8", "integer literal out of range for u8"),
        ("x: u8 = 0x100", "integer literal out of range for u8"),
        ("x = 0x80i8", "integer literal out of range for i8"),
        (
            "x = 0x8000_0000_0000_0000",
            "integer literal out of range for i64",
        ),
        (
            "x = -0x1u8",
            "unary minus is not defined for unsigned type u8",
        ),
    ] {
        let message = rejected(source);
        assert!(message.contains(error), "{source}\n{message}");
    }
}

#[test]
fn malformed_radix_integers_are_rejected_at_the_literal() {
    for (source, error) in [
        ("x = 0x", "3:9: `0x` needs at least one hexadecimal digit"),
        ("x = 0b102", "3:9: invalid digit `2` in a binary literal"),
        ("x = 0o19", "3:9: invalid digit `9` in an octal literal"),
        (
            "x = 0xFG",
            "3:9: invalid digit `G` in a hexadecimal literal",
        ),
        ("x = 0b1f32", "3:9: invalid digit `f` in a binary literal"),
        ("x = 0x1.5", "3:"),
        ("x = 0x1u7", "3:9: invalid integer suffix"),
        ("x = 0x_1", "3:9: invalid integer separator"),
        (
            "x = 0x1_0000_0000_0000_0000_0000_0000_0000_0000",
            "3:9: integer literal out of range",
        ),
    ] {
        let message = rejected(source);
        assert!(message.contains(error), "{source}\n{message}");
    }
}

#[test]
fn unicode_escapes_name_scalars_in_strings_and_f_strings() {
    support::assert_stdout(
        r#"
def main() -> Result[(), IoError | AllocError]:
    print(len("e\u{301}"), "e\u{301}" == "e\u{0301}", "\u{1F600}".byte_len())?
    print("\u{63}af\u{E9}", '\u{10ffff}'.byte_len(), """a\u{200d}b""".byte_len())?
    name = "x"
    print(f"\u{e9}{name}\u{2603}"?)?
    print(f"{'caf\u{e9} \u{1F600}'!a}"?)?
    print("caf\u{e9} \u{1f600}" == "café 😀")?
    Ok(())
"#,
        "2 True 4\ncafé 4 5\néx☃\n\"caf\\u{e9} \\u{1f600}\"\nTrue\n",
    );
}

#[test]
fn malformed_unicode_escapes_are_rejected() {
    let shape = "`\\u` needs 1 to 6 hexadecimal digits in braces, as in `\\u{200d}`";
    for (source, error) in [
        ("x = \"\\u200d\"", format!("3:10: {shape}")),
        ("x = \"\\u{}\"", format!("3:10: {shape}")),
        ("x = \"\\u{1234567}\"", format!("3:10: {shape}")),
        ("x = \"\\u{12\"", format!("3:10: {shape}")),
        ("x = f\"\\u{_1}\"", format!("3:11: {shape}")),
        (
            "x = \"\\u{D800}\"",
            "3:10: `\\u{D800}` is a surrogate code point, not a Unicode scalar value".into(),
        ),
        (
            "x = f\"\\u{dfff}\"",
            "3:11: `\\u{dfff}` is a surrogate code point, not a Unicode scalar value".into(),
        ),
        (
            "x = \"\\u{110000}\"",
            "3:10: `\\u{110000}` is beyond the largest Unicode scalar value, `\\u{10ffff}`"
                .into(),
        ),
        ("x = \"\\U0001F600\"", "3:10: unsupported escape \\U".into()),
        (
            "x = b\"\\u{41}\"",
            "unsupported escape \\u in a byte literal; a byte literal holds bytes, not scalars, so write the UTF-8 encoding with `\\xHH` escapes".into(),
        ),
    ] {
        let message = rejected(source);
        assert!(message.contains(&error), "{source}\n{message}");
    }
}

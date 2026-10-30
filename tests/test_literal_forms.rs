//! Hexadecimal, octal, and binary integer literals (issue #563) and the
//! `\u{H...}` string escape (issue #564).
mod support;
use support::check_error as rejected;

#[test]
fn radix_integers_have_decimal_values_suffixes_and_context_types() {
    support::assert_stdout(
        r#"
const FLAGS: u8 = 0b1000_0001
def main() -> Result[(), IoError]:
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
from std.fmt import FormatError
def main() -> Result[(), IoError | FormatError]:
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

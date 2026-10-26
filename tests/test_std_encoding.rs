//! `std.encoding`: canonical binary codecs and integer packing over `Bytes`.
mod support;
use support::{check_file_error as rejected, hex};

const HEADER: &str = "import std.bytes\nimport std.encoding\nfrom std.bytes import Bytes\nfrom std.encoding import Endian, EncodingError\n";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.encoding.EncodingError]:\n{body}    Ok(())\n"
    )
}

/// Runs a program and shortens the qualified names in rendered results.
fn output(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.encoding.EncodingError", "E")
        .replace("std.bytes.Bytes", "Bytes")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// Independent reference encoder for known-answer comparison.
fn reference_base64(data: &[u8], url: bool) -> String {
    let alphabet: Vec<char> = if url {
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    }
    .chars()
    .collect();
    let mut text = String::new();
    for chunk in data.chunks(3) {
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let bits = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        for position in 0..4 {
            if position <= chunk.len() {
                text.push(alphabet[(bits >> (18 - 6 * position) & 63) as usize]);
            } else if !url {
                text.push('=');
            }
        }
    }
    text
}

// RFC 4648 section 10.
const VECTORS: [(&str, &str); 7] = [
    ("", ""),
    ("f", "Zg=="),
    ("fo", "Zm8="),
    ("foo", "Zm9v"),
    ("foob", "Zm9vYg=="),
    ("fooba", "Zm9vYmE="),
    ("foobar", "Zm9vYmFy"),
];

#[test]
fn base32_rfc_vectors_cover_every_partial_group() {
    let vectors = [
        ("", "", ""),
        ("f", "MY======", "CO======"),
        ("fo", "MZXQ====", "CPNG===="),
        ("foo", "MZXW6===", "CPNMU==="),
        ("foob", "MZXW6YQ=", "CPNMUOG="),
        ("fooba", "MZXW6YTB", "CPNMUOJ1"),
        ("foobar", "MZXW6YTBOI======", "CPNMUOJ1E8======"),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (n, (plain, base32, extended)) in vectors.into_iter().enumerate() {
        body.push_str(&format!(
            r#"
data{n} = std.bytes.from_text("{plain}")?
print("[", std.encoding.base32_text(&data{n}, 0, data{n}.len())?, "]")?
print("[", std.encoding.base32hex_text(&data{n}, 0, data{n}.len())?, "]")?
print(std.encoding.base32_from_text("{base32}")? == data{n}, std.encoding.base32hex_from_text("{extended}")? == data{n})?
"#
        ));
        expected.push_str(&format!("[ {base32} ]\n[ {extended} ]\nTrue True\n"));
    }
    native(&body, &expected);
}

// An intentionally different reference: materialize a bit string, then read
// fixed-width chunks. Production code keeps only a small pending-bit value.
fn reference_radix(data: &[u8], alphabet: &str, padding: Option<char>) -> String {
    let width = alphabet.len().ilog2() as usize;
    let bits: String = data.iter().map(|byte| format!("{byte:08b}")).collect();
    let mut out = String::new();
    for chunk in bits.as_bytes().chunks(width) {
        let text = std::str::from_utf8(chunk).unwrap();
        let value = usize::from_str_radix(text, 2).unwrap() << (width - chunk.len());
        out.push(alphabet.as_bytes()[value] as char);
    }
    if let Some(pad) = padding {
        while !(out.len() * width).is_multiple_of(8) {
            out.push(pad);
        }
    }
    out
}

#[test]
fn custom_radices_match_independent_encoding_and_roundtrip_every_length() {
    let alphabets = [
        "01",
        "ACTG",
        "76543210",
        "fedcba9876543210",
        "abcdefghijklmnopqrstuvwxyz234567",
        "./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
    ];
    let data: Vec<u8> = (0..=255).collect();
    let mut body = String::from(
        "mut data = std.bytes.with_capacity(256)?\nfor n in range(256):\n    data.push(u8(n))?\nmut failures = 0\n",
    );
    let mut expected = String::new();
    for (n, alphabet) in alphabets.into_iter().enumerate() {
        body.push_str(&format!(
            r#"
padded{n} = std.encoding.custom("{alphabet}", Some(126u8))?
bare{n} = std.encoding.custom("{alphabet}", Nothing)?
for stop in range(257):
    start = stop % 4
    if start <= stop:
        plain = data.slice(start, stop)?
        encoded = padded{n}.encode(&data, start, stop)?
        unpadded = bare{n}.encode(&data, start, stop)?
        decoded = padded{n}.decode(&encoded, 0, encoded.len())?
        if decoded != plain or bare{n}.decode(&unpadded, 0, unpadded.len())? != plain:
            failures += 1
        if encoded.len() != encoded.capacity() or decoded.len() != decoded.capacity():
            failures += 1
print(padded{n}.text(&data, 1, data.len())?)?
print(bare{n}.text(&data, 1, data.len())?)?
"#
        ));
        expected.push_str(&reference_radix(&data[1..], alphabet, Some('~')));
        expected.push('\n');
        expected.push_str(&reference_radix(&data[1..], alphabet, None));
        expected.push('\n');
    }
    body.push_str(
        r#"
for stop in range(257):
    plain = data.slice(0, stop)?
    a = std.encoding.base32_encode(&plain, 0, plain.len())?
    b = std.encoding.base32hex_encode(&plain, 0, plain.len())?
    if std.encoding.base32_decode(&a, 0, a.len())? != plain or std.encoding.base32hex_decode(&b, 0, b.len())? != plain:
        failures += 1
print(failures)?
"#,
    );
    expected.push('0');
    native(&body, &expected);
}

#[test]
fn base32_and_custom_decoders_reject_ambiguous_encodings_and_preserve_indexes() {
    let cases = [
        ("MY", "InvalidLength"),
        ("M=======", "InvalidLength"),
        ("MZX=====", "InvalidLength"),
        ("MZXW6Y==", "InvalidLength"),
        ("========", "InvalidPadding(0)"),
        ("MY==============", "InvalidPadding(2)"),
        ("M=Y=====", "InvalidLength"),
        ("MY======MY======", "InvalidPadding(2)"),
        ("my======", "InvalidByte(0)"),
        ("M0======", "InvalidByte(1)"),
        ("MZ======", "NonCanonical(1)"),
        ("MZXR====", "NonCanonical(3)"),
        ("MZXW7===", "NonCanonical(4)"),
        ("MZXW6YR=", "NonCanonical(6)"),
        ("M ======", "InvalidByte(1)"),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (input, error) in cases {
        body.push_str(&format!(
            "print(str.repr(std.encoding.base32_from_text(\"{input}\"))?)?\n"
        ));
        expected.push_str(&format!("Result[Bytes, E].Err(E.{error})\n"));
    }
    body.push_str(
        r#"
frame = std.bytes.from_text("..MZ======..")?
print(str.repr(std.encoding.base32_decode(&frame, 2, 10))?)?
print(str.repr(std.encoding.base32_decode(&frame, -1, 0))?)?
print(str.repr(std.encoding.base32_encode(&frame, 0, 99))?)?
print(str.repr(std.encoding.base32hex_from_text("CP======"))?)?
print(str.repr(std.encoding.base32hex_from_text("CW======"))?)?
codec = std.encoding.custom("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567", Nothing)?
print(codec.from_text("MY")?.to_text()?)?
print(str.repr(codec.from_text("MZ"))?)?
print(str.repr(codec.from_text("MY======"))?)?
custom_pad = std.encoding.custom("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567", Some(126u8))?
print(custom_pad.from_text("MY~~~~~~")?.to_text()?)?
print(str.repr(custom_pad.from_text("MY======"))?)?
"#,
    );
    expected.push_str("Result[Bytes, E].Err(E.NonCanonical(3))\nResult[Bytes, E].Err(E.OutOfRange)\nResult[Bytes, E].Err(E.OutOfRange)\nResult[Bytes, E].Err(E.NonCanonical(1))\nResult[Bytes, E].Err(E.InvalidByte(1))\nf\nResult[Bytes, E].Err(E.NonCanonical(1))\nResult[Bytes, E].Err(E.InvalidByte(2))\nf\nResult[Bytes, E].Err(E.InvalidByte(2))");
    native(&body, &expected);
}

#[test]
fn custom_alphabets_are_validated_before_use() {
    native(
        r#"
print(std.encoding.custom("").is_err(), std.encoding.custom("0").is_err(), std.encoding.custom("012").is_err())?
print(std.encoding.custom("00").is_err(), std.encoding.custom("0 ").is_err(), std.encoding.custom("0\n").is_err())?
print(std.encoding.custom("0é").is_err(), std.encoding.custom("0=", Some(61u8)).is_err())?
print(std.encoding.custom("01", Some(32u8)).is_err(), std.encoding.custom("01", Some(128u8)).is_err())?
print(std.encoding.custom("0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz+-_=").is_err())?
codec = std.encoding.custom("=x", Nothing)?
value = std.bytes.from_list(&[129u8]?)?
print(codec.text(&value, 0, 1)?, codec.from_text("x======x")? == value)?
print(std.encoding.custom("0123456789ABCDEF").is_ok())?
"#,
        "True True True\nTrue True True\nTrue True\nTrue True\nTrue\nx======x True\nTrue",
    );
}

#[test]
fn mime_wraps_at_76_columns_and_only_ignores_explicit_whitespace() {
    let data: Vec<u8> = (0..=255).collect();
    let mut body = String::from(
        "mut data = std.bytes.with_capacity(256)?\nfor n in range(256):\n    data.push(u8(n))?\n",
    );
    let mut expected = String::new();
    for stop in [0, 1, 56, 57, 58, 113, 114, 115, 256] {
        let encoded = reference_base64(&data[..stop], false);
        let wrapped = encoded
            .as_bytes()
            .chunks(76)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join("\r\n");
        body.push_str(&format!(
            r#"
wrapped{stop} = std.encoding.base64mime_encode(&data, 0, {stop})?
print("[", std.encoding.hex_text(&wrapped{stop}, 0, wrapped{stop}.len())?, "]")?
print(wrapped{stop}.capacity() == wrapped{stop}.len(), std.encoding.base64mime_decode(&wrapped{stop}, 0, wrapped{stop}.len())? == data.slice(0, {stop})?)?
"#
        ));
        expected.push_str(&format!("[ {} ]\nTrue True\n", hex(wrapped.as_bytes())));
    }
    body.push_str(
        r#"
print(std.encoding.base64mime_from_text(" \tZ m\r\n9 v\n")?.to_text()?)?
print(std.encoding.base64mime_from_text("\t \r\n")?.len())?
print(str.repr(std.encoding.base64mime_from_text("Z!g=="))?)?
print(str.repr(std.encoding.base64mime_from_text("Z\tg= =A==="))?)?
print(str.repr(std.encoding.base64mime_from_text(" \tZ h==\r\n"))?)?
print(str.repr(std.encoding.base64mime_from_text("Zg"))?)?
frame = std.bytes.from_text(".. Z !g= ..")?
print(str.repr(std.encoding.base64mime_decode(&frame, 2, 9))?)?
print(str.repr(std.encoding.base64mime_decode(&frame, 9, 2))?)?
print(std.encoding.base64mime_text(&data, 0, 1)?)?
"#,
    );
    expected.push_str("foo\n0\nResult[Bytes, E].Err(E.InvalidLength)\nResult[Bytes, E].Err(E.InvalidPadding(3))\nResult[Bytes, E].Err(E.NonCanonical(4))\nResult[Bytes, E].Err(E.InvalidLength)\nResult[Bytes, E].Err(E.InvalidByte(5))\nResult[Bytes, E].Err(E.OutOfRange)\nAA==");
    native(&body, &expected);
}

#[test]
fn rfc_4648_vectors_encode_and_decode() {
    let mut body = String::new();
    let mut expected = String::new();
    for (n, (plain, coded)) in VECTORS.into_iter().enumerate() {
        let unpadded = coded.trim_end_matches('=');
        let digits = hex(plain.as_bytes());
        body.push_str(&format!(
            r#"
plain{n} = std.bytes.from_text("{plain}")?
print("[", std.encoding.base64_text(&plain{n}, 0, plain{n}.len())?, "]")?
print("[", std.encoding.base64url_text(&plain{n}, 0, plain{n}.len())?, "]")?
print("[", std.encoding.hex_text(&plain{n}, 0, plain{n}.len())?, "]")?
print("[", std.encoding.base64_from_text("{coded}")?.to_text()?, "]")?
print("[", std.encoding.base64url_from_text("{unpadded}")?.to_text()?, "]")?
print("[", std.encoding.hex_from_text("{digits}")?.to_text()?, "]")?
print("[", std.encoding.hex_from_text("{upper}")?.to_text()?, "]")?
"#,
            upper = digits.to_uppercase(),
        ));
        expected.push_str(&format!(
            "[ {coded} ]\n[ {unpadded} ]\n[ {digits} ]\n[ {plain} ]\n[ {plain} ]\n[ {plain} ]\n[ {plain} ]\n"
        ));
    }
    native(&body, &expected);
}

#[test]
fn alphabets_differ_only_in_the_last_two_symbols_and_padding() {
    native(
        r#"
data = std.bytes.from_list(&[251u8, 255u8, 254u8]?)?
print(std.encoding.base64_text(&data, 0, 3)?, std.encoding.base64url_text(&data, 0, 3)?)?
print(std.encoding.base64_text(&data, 0, 2)?, std.encoding.base64url_text(&data, 0, 2)?)?
print(std.encoding.base64_text(&data, 0, 1)?, std.encoding.base64url_text(&data, 0, 1)?)?
print(std.encoding.base64_from_text("+//+")? == data, std.encoding.base64url_from_text("-__-")? == data)?
print(str.repr(std.encoding.base64_from_text("-__-"))?)?
print(str.repr(std.encoding.base64url_from_text("+//+"))?)?
encoded = std.encoding.base64_encode(&data, 0, 2)?
print(encoded.len(), encoded.capacity(), encoded.to_text()?)?
"#,
        r#"
+//+ -__-
+/8= -_8
+w== -w
True True
Result[Bytes, E].Err(E.InvalidByte(0))
Result[Bytes, E].Err(E.InvalidByte(0))
4 4 +/8=
"#,
    );
}

#[test]
fn every_byte_value_matches_the_reference_at_every_length() {
    let data: Vec<u8> = (0..=255u8)
        .chain((0..70u32).map(|n| ((n * 37 + 11) % 256) as u8))
        .collect();
    let expected = format!(
        "0\n{}\n{}\n{}\n{}\n{}",
        reference_base64(&data, false),
        reference_base64(&data, true),
        hex(&data),
        reference_base64(&data[1..], false),
        reference_base64(&data[2..], true),
    );
    native(
        r#"
mut data = std.bytes.with_capacity(326)?
for n in range(256):
    data.push(u8(n))?
for n in range(70):
    data.push(u8((n * 37 + 11) % 256))?
mut failures = 0
for stop in range(data.len() + 1):
    start = stop % 4
    if start <= stop:
        plain = data.slice(start, stop)?
        digits = std.encoding.hex_encode(&data, start, stop)?
        padded = std.encoding.base64_encode(&data, start, stop)?
        bare = std.encoding.base64url_encode(&data, start, stop)?
        from_digits = std.encoding.hex_decode(&digits, 0, digits.len())?
        from_padded = std.encoding.base64_decode(&padded, 0, padded.len())?
        from_bare = std.encoding.base64url_decode(&bare, 0, bare.len())?
        if from_digits != plain or from_padded != plain or from_bare != plain:
            failures += 1
        if digits.len() != digits.capacity() or padded.len() != padded.capacity():
            failures += 1
        if bare.len() != bare.capacity() or from_padded.len() != from_padded.capacity():
            failures += 1
        if padded.len() % 4 != 0 or digits.len() != plain.len() * 2:
            failures += 1
print(failures)?
print(std.encoding.base64_text(&data, 0, 326)?)?
print(std.encoding.base64url_text(&data, 0, 326)?)?
print(std.encoding.hex_text(&data, 0, 326)?)?
print(std.encoding.base64_text(&data, 1, 326)?)?
print(std.encoding.base64url_text(&data, 2, 326)?)?
"#,
        &expected,
    );
}

#[test]
fn malformed_hex_is_rejected_with_the_offending_index() {
    native(
        r#"
print(str.repr(std.encoding.hex_from_text("abc"))?)?
print(str.repr(std.encoding.hex_from_text("0g"))?)?
print(str.repr(std.encoding.hex_from_text("00x0"))?)?
print(str.repr(std.encoding.hex_from_text("0x00"))?)?
print(str.repr(std.encoding.hex_from_text("00 11"))?)?
print(str.repr(std.encoding.hex_from_text("00 1"))?)?
print(str.repr(std.encoding.hex_from_text("00\n"))?)?
print(str.repr(std.encoding.hex_from_text("@0"))?)?
print(str.repr(std.encoding.hex_from_text("0G"))?)?
print(str.repr(std.encoding.hex_from_text("0`"))?)?
print(str.repr(std.encoding.hex_from_text("/0"))?)?
print(str.repr(std.encoding.hex_from_text(":0"))?)?
high = std.bytes.from_list(&[48u8, 200u8]?)?
print(str.repr(std.encoding.hex_decode(&high, 0, 2))?)?
print(std.encoding.hex_from_text("aAfF09")? == std.bytes.from_list(&[170u8, 255u8, 9u8]?)?)?
"#,
        r#"
Result[Bytes, E].Err(E.InvalidLength)
Result[Bytes, E].Err(E.InvalidByte(1))
Result[Bytes, E].Err(E.InvalidByte(2))
Result[Bytes, E].Err(E.InvalidByte(1))
Result[Bytes, E].Err(E.InvalidLength)
Result[Bytes, E].Err(E.InvalidByte(2))
Result[Bytes, E].Err(E.InvalidLength)
Result[Bytes, E].Err(E.InvalidByte(0))
Result[Bytes, E].Err(E.InvalidByte(1))
Result[Bytes, E].Err(E.InvalidByte(1))
Result[Bytes, E].Err(E.InvalidByte(0))
Result[Bytes, E].Err(E.InvalidByte(0))
Result[Bytes, E].Err(E.InvalidByte(1))
True
"#,
    );
}

#[test]
fn malformed_base64_is_rejected_by_length_alphabet_padding_and_trailing_bits() {
    let cases = [
        // Padded form: the length must be a multiple of four.
        ("base64", "Zg", "InvalidLength"),
        ("base64", "Zg=", "InvalidLength"),
        ("base64", "Zm9vY", "InvalidLength"),
        ("base64", "Zm9v\n", "InvalidLength"),
        // Nothing outside the alphabet is skipped, including whitespace.
        ("base64", "Zm 9", "InvalidByte(2)"),
        ("base64", "Zm9v\nZg=", "InvalidByte(4)"),
        ("base64", "Zm9v\r\nZg==", "InvalidLength"),
        ("base64", "Zm9vZg=\n", "InvalidPadding(6)"),
        ("base64", "Zm_v", "InvalidByte(2)"),
        ("base64", "Zm9*", "InvalidByte(3)"),
        ("base64", "@m9v", "InvalidByte(0)"),
        ("base64", "[m9v", "InvalidByte(0)"),
        ("base64", "`m9v", "InvalidByte(0)"),
        ("base64", "{m9v", "InvalidByte(0)"),
        ("base64", ":m9v", "InvalidByte(0)"),
        ("base64", ".m9v", "InvalidByte(0)"),
        // `=` is valid only as one or two final symbols.
        ("base64", "====", "InvalidPadding(0)"),
        ("base64", "Z===", "InvalidPadding(1)"),
        ("base64", "Zg=a", "InvalidPadding(2)"),
        ("base64", "=Zg=", "InvalidPadding(0)"),
        ("base64", "Zg==Zg==", "InvalidPadding(2)"),
        ("base64", "Zm8=Zm9v", "InvalidPadding(3)"),
        // Bits that encode no byte must be zero.
        ("base64", "Zh==", "NonCanonical(1)"),
        ("base64", "Zm9=", "NonCanonical(2)"),
        ("base64", "Zm9vZm9=", "NonCanonical(6)"),
        // Unpadded form.
        ("base64url", "Z", "InvalidLength"),
        ("base64url", "Zm9vY", "InvalidLength"),
        ("base64url", "Zg==", "InvalidPadding(2)"),
        ("base64url", "Zm8=", "InvalidPadding(3)"),
        ("base64url", "Zg=", "InvalidPadding(2)"),
        ("base64url", "Z+", "InvalidByte(1)"),
        ("base64url", "Z/", "InvalidByte(1)"),
        ("base64url", "Zm 9", "InvalidByte(2)"),
        ("base64url", "Zh", "NonCanonical(1)"),
        ("base64url", "Zm9", "NonCanonical(2)"),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (codec, text, error) in cases {
        let literal = text.replace('\n', "\\n").replace('\r', "\\r");
        body.push_str(&format!(
            "print(str.repr(std.encoding.{codec}_from_text(\"{literal}\"))?)?\n"
        ));
        expected.push_str(&format!("Result[Bytes, E].Err(E.{error})\n"));
    }
    body.push_str(
        r#"
high = std.bytes.from_list(&[90u8, 200u8, 61u8, 61u8]?)?
print(str.repr(std.encoding.base64_decode(&high, 0, 4))?)?
print(str.repr(std.encoding.base64url_decode(&high, 0, 2))?)?
"#,
    );
    expected.push_str(
        "Result[Bytes, E].Err(E.InvalidByte(1))\nResult[Bytes, E].Err(E.InvalidByte(1))\n",
    );
    native(&body, &expected);
}

#[test]
fn ranges_follow_the_bytes_rule_and_errors_use_buffer_indexes() {
    native(
        r#"
data = std.bytes.from_text("..foobar..")?
print(std.encoding.hex_text(&data, 2, 5)?, std.encoding.base64_text(&data, 2, 8)?)?
print(std.encoding.base64url_text(&data, 2, 6)?)?
print("[", std.encoding.hex_text(&data, 10, 10)?, std.encoding.base64_text(&data, 0, 0)?, "]")?
print(str.repr(std.encoding.hex_encode(&data, 0, 11))?)?
print(str.repr(std.encoding.hex_encode(&data, -1, 4))?)?
print(str.repr(std.encoding.hex_encode(&data, 5, 4))?)?
print(str.repr(std.encoding.base64_encode(&data, 0, 11))?)?
print(str.repr(std.encoding.base64url_encode(&data, -1, 4))?)?
print(str.repr(std.encoding.base64_text(&data, 11, 11))?)?
print(str.repr(std.encoding.hex_text(&data, 9223372036854775807, 9223372036854775807))?)?
print(str.repr(std.encoding.hex_decode(&data, 0, 12))?)?
print(str.repr(std.encoding.base64_decode(&data, -4, 0))?)?
print(str.repr(std.encoding.base64url_decode(&data, 6, 5))?)?
framed = std.bytes.from_text("<<Zm9vYg==>>6f6b!!")?
print(std.encoding.base64_decode(&framed, 2, 10)?.to_text()?)?
print(std.encoding.base64url_decode(&framed, 2, 8)?.to_text()?)?
print(std.encoding.hex_decode(&framed, 12, 16)?.to_text()?)?
print(str.repr(std.encoding.base64_decode(&framed, 0, 4))?)?
print(str.repr(std.encoding.base64_decode(&framed, 6, 14))?)?
print(str.repr(std.encoding.base64url_decode(&framed, 6, 9))?)?
print(str.repr(std.encoding.hex_decode(&framed, 14, 18))?)?
print(std.encoding.hex_decode(&framed, 18, 18)?.len(), std.encoding.base64_decode(&framed, 3, 3)?.len())?
"#,
        r#"
666f6f Zm9vYmFy
Zm9vYg
[   ]
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[str, E].Err(E.OutOfRange)
Result[str, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
Result[Bytes, E].Err(E.OutOfRange)
foob
foob
ok
Result[Bytes, E].Err(E.InvalidByte(0))
Result[Bytes, E].Err(E.InvalidPadding(8))
Result[Bytes, E].Err(E.InvalidPadding(8))
Result[Bytes, E].Err(E.InvalidByte(16))
0 0
"#,
    );
}

#[test]
fn integers_pack_and_unpack_in_both_byte_orders() {
    let mut body = String::from("mut out = std.bytes.empty()?\n");
    let mut expected = String::new();
    let mut emit = |ty: &str, value: String, big: Vec<u8>| {
        let width = big.len();
        let little: Vec<u8> = big.iter().rev().copied().collect();
        body.push_str(&format!(
            r#"
out.clear()
std.encoding.append_{ty}(&mut out, {value}{ty}, Endian.Big)?
std.encoding.append_{ty}(&mut out, {value}{ty}, Endian.Little)?
out.resize({total}, 0u8)?
std.encoding.pack_{ty}(&mut out, {second}, {value}{ty}, Endian.Little)?
std.encoding.pack_{ty}(&mut out, {third}, {value}{ty}, Endian.Big)?
print(std.encoding.hex_text(&out, 0, out.len())?)?
print(std.encoding.unpack_{ty}(&out, 0, Endian.Big)?, std.encoding.unpack_{ty}(&out, {width}, Endian.Little)?, std.encoding.unpack_{ty}(&out, {third}, Endian.Big)?)?
"#,
            total = width * 4,
            second = width * 2,
            third = width * 3,
        ));
        expected.push_str(&format!(
            "{}{}{}{}\n{value} {value} {value}\n",
            hex(&big),
            hex(&little),
            hex(&little),
            hex(&big)
        ));
    };
    for value in [0u16, 1, 258, 0x8000, u16::MAX] {
        emit("u16", value.to_string(), value.to_be_bytes().to_vec());
    }
    for value in [0u32, 1, 0x0102_0304, 0xdead_beef, u32::MAX] {
        emit("u32", value.to_string(), value.to_be_bytes().to_vec());
    }
    for value in [0u64, 1, 0x0102_0304_0506_0708, 1 << 63, u64::MAX] {
        emit("u64", value.to_string(), value.to_be_bytes().to_vec());
    }
    for value in [0i16, 1, -1, -2, 0x0102, i16::MIN, i16::MAX] {
        emit("i16", value.to_string(), value.to_be_bytes().to_vec());
    }
    for value in [0i32, 1, -1, -2, 0x0102_0304, i32::MIN, i32::MAX] {
        emit("i32", value.to_string(), value.to_be_bytes().to_vec());
    }
    for value in [0i64, 1, -1, -2, 0x0102_0304_0506_0708, i64::MIN, i64::MAX] {
        emit("i64", value.to_string(), value.to_be_bytes().to_vec());
    }
    native(&body, &expected);
}

#[test]
fn byte_order_changes_the_value_read_from_the_same_bytes() {
    native(
        r#"
data = std.encoding.hex_from_text("0102030405060708ff")?
print(std.encoding.unpack_u16(&data, 0, Endian.Big)?, std.encoding.unpack_u16(&data, 0, Endian.Little)?)?
print(std.encoding.unpack_u32(&data, 0, Endian.Big)?, std.encoding.unpack_u32(&data, 0, Endian.Little)?)?
print(std.encoding.unpack_u64(&data, 0, Endian.Big)?, std.encoding.unpack_u64(&data, 0, Endian.Little)?)?
print(std.encoding.unpack_u16(&data, 7, Endian.Big)?, std.encoding.unpack_i16(&data, 7, Endian.Big)?)?
print(std.encoding.unpack_u16(&data, 7, Endian.Little)?, std.encoding.unpack_i16(&data, 7, Endian.Little)?)?
print(std.encoding.unpack_i32(&data, 5, Endian.Little)?, std.encoding.unpack_i64(&data, 1, Endian.Little)?)?
"#,
        &format!(
            "258 513\n16909060 67305985\n{} {}\n2303 2303\n65288 -248\n{} {}",
            0x0102_0304_0506_0708u64,
            0x0807_0605_0403_0201u64,
            i32::from_le_bytes([6, 7, 8, 0xff]),
            i64::from_le_bytes([2, 3, 4, 5, 6, 7, 8, 0xff]),
        ),
    );
}

#[test]
fn short_buffers_and_bad_offsets_are_out_of_range_and_change_nothing() {
    native(
        r#"
mut buffer = std.encoding.hex_from_text("00112233445566")?
room = buffer.capacity()
print(str.repr(std.encoding.unpack_u16(&buffer, 6, Endian.Big))?)?
print(str.repr(std.encoding.unpack_u16(&buffer, -1, Endian.Big))?)?
print(str.repr(std.encoding.unpack_i32(&buffer, 4, Endian.Little))?)?
print(str.repr(std.encoding.unpack_u64(&buffer, 0, Endian.Little))?)?
print(str.repr(std.encoding.unpack_i64(&buffer, 9223372036854775807, Endian.Little))?)?
print(str.repr(std.encoding.unpack_i16(&buffer, -9223372036854775808, Endian.Little))?)?
print(str.repr(std.encoding.pack_u16(&mut buffer, 6, 65535u16, Endian.Big))?)?
print(str.repr(std.encoding.pack_i16(&mut buffer, 7, -1i16, Endian.Big))?)?
print(str.repr(std.encoding.pack_u32(&mut buffer, 4, 4294967295u32, Endian.Little))?)?
print(str.repr(std.encoding.pack_i32(&mut buffer, -1, -1i32, Endian.Little))?)?
print(str.repr(std.encoding.pack_u64(&mut buffer, 0, 1u64, Endian.Big))?)?
print(str.repr(std.encoding.pack_i64(&mut buffer, 9223372036854775807, 1, Endian.Big))?)?
print(std.encoding.hex_text(&buffer, 0, buffer.len())?, buffer.capacity() == room)?
print(std.encoding.unpack_u16(&buffer, 5, Endian.Big)?, std.encoding.unpack_u32(&buffer, 3, Endian.Big)?)?
std.encoding.pack_u16(&mut buffer, 5, 43981u16, Endian.Big)?
std.encoding.pack_u32(&mut buffer, 0, 1u32, Endian.Little)?
print(std.encoding.hex_text(&buffer, 0, buffer.len())?, buffer.len())?
empty = std.bytes.empty()?
print(str.repr(std.encoding.unpack_u16(&empty, 0, Endian.Big))?)?
"#,
        r#"
Result[u16, E].Err(E.OutOfRange)
Result[u16, E].Err(E.OutOfRange)
Result[i32, E].Err(E.OutOfRange)
Result[u64, E].Err(E.OutOfRange)
Result[i64, E].Err(E.OutOfRange)
Result[i16, E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
Result[(), E].Err(E.OutOfRange)
00112233445566 True
21862 860116326
0100000044abcd 7
Result[u16, E].Err(E.OutOfRange)
"#,
    );
}

#[test]
fn a_view_borrow_blocks_packing_into_its_owner() {
    let error = rejected(
        r#"
import std.bytes
import std.encoding
from std.encoding import Endian

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.encoding.EncodingError]:
    mut frame = std.bytes.from_text("abcd")?
    view = &frame
    std.encoding.pack_u16(&mut frame, 0, 1u16, Endian.Big)?
    print(std.encoding.unpack_u16(view, 0, Endian.Big)?)?
    Ok(())
"#,
    );
    assert!(error.contains("conflicting borrow"), "{error}");
}

#[test]
fn codecs_run_in_worker_threads() {
    let source = format!(
        "{HEADER}{}",
        r#"
def work(data: Bytes) -> ():
    encoded = std.encoding.base64_encode(&data, 0, data.len()).unwrap()
    print(encoded.to_text().unwrap(), std.encoding.unpack_u16(&data, 0, Endian.Big).unwrap()).unwrap()

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.encoding.EncodingError | ThreadError]:
    data = std.bytes.from_text("foobar")?
    with spawn(work, data)?:
        pass
    Ok(())
"#
    );
    assert_eq!(output(&source), "Zm9vYmFy 26223");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn packing_and_unpacking_within_length_do_not_allocate() {
    native(
        r#"
mut buffer = std.bytes.with_capacity(32)?
buffer.resize(16, 0u8)?
print("__test_fail_allocations_after_0__")?
print("__test_begin_no_allocations__")?
std.encoding.pack_u64(&mut buffer, 0, 72623859790382856u64, Endian.Big)?
std.encoding.pack_i32(&mut buffer, 8, -2i32, Endian.Little)?
std.encoding.append_u16(&mut buffer, 258u16, Endian.Big)?
std.encoding.append_i64(&mut buffer, -1, Endian.Little)?
a = std.encoding.unpack_u64(&buffer, 0, Endian.Little)?
b = std.encoding.unpack_i32(&buffer, 8, Endian.Little)?
c = std.encoding.unpack_u16(&buffer, 16, Endian.Big)?
short = std.encoding.unpack_u64(&buffer, 20, Endian.Big)
print("__test_restore_allocations__")?
print("__test_end_no_allocations__")?
print(a, b, c, buffer.len(), buffer.capacity(), str.repr(short)?)?
"#,
        "578437695752307201 -2 258 26 32 Result[u64, E].Err(E.OutOfRange)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn extended_codecs_propagate_every_allocation_failure() {
    let source = format!(
        "{HEADER}{}",
        r#"
import std.fmt

def exercise(data: &Bytes) -> Result[(), EncodingError]:
    codec = std.encoding.custom("abcdefghijklmnopqrstuvwxyz234567", Nothing)?
    a = std.encoding.base32_encode(data, 0, data.len())?
    drop(std.encoding.base32_decode(&a, 0, a.len())?)
    b = std.encoding.base32hex_text(data, 0, data.len())?
    drop(std.encoding.base32hex_from_text(&b)?)
    c = std.encoding.base64mime_text(data, 0, data.len())?
    drop(std.encoding.base64mime_from_text(&c)?)
    d = codec.text(data, 0, data.len())?
    drop(codec.from_text(&d)?)
    Ok(())

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | EncodingError | std.fmt.FormatError]:
    data = std.bytes.from_text("a long input that exercises heap strings and MIME line wrapping across its seventy-six-column boundary")?
    mut failed = 0
    mut succeeded = 0
    for budget in range(50):
        print(f"__test_fail_allocations_after_{budget}__"?)?
        result = exercise(&data)
        print("__test_restore_allocations__")?
        match result:
            case Ok(_):
                succeeded += 1
            case Err(error):
                match error:
                    case EncodingError.Allocation(_):
                        failed += 1
                    case _:
                        return Err(error)
    print(failed > 0, succeeded > 0, data.len())?
    Ok(())
"#
    );
    assert_eq!(output(&source), "True True 102");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_and_leaves_buffers_unchanged() {
    let failure = "E].Err(E.Allocation(AllocError.OutOfMemory))";
    let expected = format!(
        "Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[str, {failure}\n\
         Result[str, {failure}\n\
         Result[str, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[(), {failure}\n\
         Result[(), {failure}\n\
         4 4 abcd"
    );
    // Results are bound while injection is active and rendered afterwards,
    // because rendering allocates. Process exit checks that nothing leaked.
    native(
        r#"
mut full = std.bytes.with_capacity(4)?
full.extend(&std.bytes.from_text("abcd")?)?
plain = std.bytes.from_text("a text too long for inline string storage")?
digits = std.encoding.hex_encode(&plain, 0, plain.len())?
padded = std.encoding.base64_encode(&plain, 0, plain.len())?
bare = std.encoding.base64url_encode(&plain, 0, plain.len())?
digits_text = digits.to_text()?
padded_text = padded.to_text()?
bare_text = bare.to_text()?
print("__test_fail_allocations_after_0__")?
a = std.encoding.hex_encode(&plain, 0, plain.len())
b = std.encoding.hex_decode(&digits, 0, digits.len())
c = std.encoding.base64_encode(&plain, 0, plain.len())
d = std.encoding.base64_decode(&padded, 0, padded.len())
e = std.encoding.base64url_encode(&plain, 0, plain.len())
f = std.encoding.base64url_decode(&bare, 0, bare.len())
g = std.encoding.hex_text(&plain, 0, plain.len())
h = std.encoding.base64_text(&plain, 0, plain.len())
i = std.encoding.base64url_text(&plain, 0, plain.len())
j = std.encoding.hex_from_text(&digits_text)
k = std.encoding.base64_from_text(&padded_text)
l = std.encoding.base64url_from_text(&bare_text)
m = std.encoding.append_u16(&mut full, 1u16, Endian.Big)
n = std.encoding.append_i64(&mut full, 1, Endian.Little)
print("__test_restore_allocations__")?
print(str.repr(a)?)?
print(str.repr(b)?)?
print(str.repr(c)?)?
print(str.repr(d)?)?
print(str.repr(e)?)?
print(str.repr(f)?)?
print(str.repr(g)?)?
print(str.repr(h)?)?
print(str.repr(i)?)?
print(str.repr(j)?)?
print(str.repr(k)?)?
print(str.repr(l)?)?
print(str.repr(m)?)?
print(str.repr(n)?)?
print(full.len(), full.capacity(), full.to_text()?)?
"#,
        &expected,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn a_later_allocation_failure_releases_the_earlier_buffer() {
    // The text forms build an intermediate buffer (two allocations: storage,
    // then owner) before the result. Failing the third allocation must free
    // the intermediate, which the exit-time balance check verifies.
    let failure = "E].Err(E.Allocation(AllocError.OutOfMemory))";
    let expected = format!(
        "Result[str, {failure}\n\
         Result[str, {failure}\n\
         Result[str, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}\n\
         Result[Bytes, {failure}"
    );
    native(
        r#"
plain = std.bytes.from_text("a text too long for inline string storage")?
digits = std.encoding.hex_text(&plain, 0, plain.len())?
padded = std.encoding.base64_text(&plain, 0, plain.len())?
bare = std.encoding.base64url_text(&plain, 0, plain.len())?
print("__test_fail_allocations_after_2__")?
a = std.encoding.hex_text(&plain, 0, plain.len())
print("__test_fail_allocations_after_2__")?
b = std.encoding.base64_text(&plain, 0, plain.len())
print("__test_fail_allocations_after_2__")?
c = std.encoding.base64url_text(&plain, 0, plain.len())
print("__test_fail_allocations_after_2__")?
d = std.encoding.hex_from_text(&digits)
print("__test_fail_allocations_after_2__")?
e = std.encoding.base64_from_text(&padded)
print("__test_fail_allocations_after_2__")?
f = std.encoding.base64url_from_text(&bare)
print("__test_restore_allocations__")?
print(str.repr(a)?)?
print(str.repr(b)?)?
print(str.repr(c)?)?
print(str.repr(d)?)?
print(str.repr(e)?)?
print(str.repr(f)?)?
"#,
        &expected,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn a_rejected_input_releases_its_output_buffer() {
    native(
        r#"
a = std.encoding.hex_from_text("00112233445566778899aabbccddeeffgg")
b = std.encoding.base64_from_text("Zm9vYmFyZm9vYmFyZm9vYmFyZm9vYmFyZm9vYmE*")
c = std.encoding.base64url_from_text("Zm9vYmFyZm9vYmFyZm9vYmFyZm9vYmFyZm9vYmF")
print(str.repr(a)?)?
print(str.repr(b)?)?
print(str.repr(c)?)?
"#,
        r#"
Result[Bytes, E].Err(E.InvalidByte(32))
Result[Bytes, E].Err(E.InvalidByte(39))
Result[Bytes, E].Err(E.NonCanonical(38))
"#,
    );
}

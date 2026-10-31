//! Token output of the lexer for every symbol spelling and string prefix
//! (issue #431).

use super::{Kind, Lexer, SYMBOLS};
use std::collections::VecDeque;

fn lex(source: &str) -> Result<Vec<Kind>, String> {
    let mut lexer = Lexer::new(source);
    let mut tokens = VecDeque::new();
    while !lexer.done {
        lexer.item(&mut tokens).map_err(|e| e.to_string())?;
    }
    Ok(tokens.into_iter().map(|token| token.kind).collect())
}

fn word(s: &str) -> Kind {
    Kind::Word(s.into())
}

fn text(s: &str) -> Kind {
    Kind::Text(s.into())
}

fn symbols(source: &str) -> Vec<&'static str> {
    lex(source)
        .unwrap()
        .into_iter()
        .filter_map(|kind| match kind {
            Kind::Symbol(s) => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn every_symbol_lexes_to_its_spelling() {
    for symbol in SYMBOLS {
        let source = match symbol {
            "(" | ")" => "x ( ) y".to_string(),
            "[" | "]" => "x [ ] y".to_string(),
            "{" | "}" => "x { } y".to_string(),
            _ => format!("x {symbol} y"),
        };
        let tokens = lex(&source).unwrap();
        assert!(
            tokens.contains(&Kind::Symbol(symbol)),
            "{source:?}: {tokens:?}"
        );
        assert_eq!(tokens.first(), Some(&word("x")), "{source:?}");
        assert_eq!(tokens[tokens.len() - 3], word("y"), "{source:?}");
        assert_eq!(tokens[tokens.len() - 2..], [Kind::Newline, Kind::Eof]);
    }
    assert_eq!(
        lex("x -> y").unwrap(),
        [
            word("x"),
            Kind::Symbol("->"),
            word("y"),
            Kind::Newline,
            Kind::Eof
        ]
    );
}

#[test]
fn adjacent_symbols_take_the_longest_listed_spelling() {
    let cases: &[(&str, &[&str])] = &[
        ("a<<=b", &["<<="]),
        ("a>>=b", &[">>="]),
        ("a//=b", &["//="]),
        ("a///b", &["//", "/"]),
        ("a->=b", &["->", "="]),
        ("a-=b", &["-="]),
        ("a+=b", &["+="]),
        ("a*=b", &["*="]),
        ("a/=b", &["/="]),
        ("a%=b", &["%="]),
        ("a**b", &["*", "*"]),
        ("a=-b", &["=", "-"]),
        ("a===b", &["==", "="]),
        ("a!==b", &["!=", "="]),
        ("a<==b", &["<=", "="]),
        ("a>==b", &[">=", "="]),
        ("a>>>b", &[">>", ">"]),
        ("a<<<b", &["<<", "<"]),
        ("a&=b", &["&="]),
        ("a|=b", &["|="]),
        ("a^=b", &["^="]),
        ("a~b", &["~"]),
        ("a?.b", &["?", "."]),
        ("a:b,c", &[":", ","]),
        ("a!b", &["!"]),
        ("a<-b", &["<", "-"]),
        ("a>-b", &[">", "-"]),
        ("a-", &["-"]),
        ("a//", &["//"]),
        ("a/", &["/"]),
        ("f(a)[b]{c}", &["(", ")", "[", "]", "{", "}"]),
    ];
    for (source, expected) in cases {
        assert_eq!(symbols(source), *expected, "{source:?}");
    }
}

#[test]
fn a_dot_before_a_digit_starts_a_number() {
    assert_eq!(
        lex("x.5 x. 5").unwrap(),
        [
            word("x"),
            Kind::Number(".5".into()),
            word("x"),
            Kind::Symbol("."),
            Kind::Number("5".into()),
            Kind::Newline,
            Kind::Eof
        ]
    );
}

#[test]
fn string_prefixes_apply_only_to_their_exact_spelling() {
    assert_eq!(
        lex("b'ab'").unwrap(),
        [
            Kind::Bytes(b"ab".as_slice().into()),
            Kind::Newline,
            Kind::Eof
        ]
    );
    assert_eq!(
        lex("b\"\"\"a\"\"\"").unwrap(),
        [
            Kind::Bytes(b"a".as_slice().into()),
            Kind::Newline,
            Kind::Eof
        ]
    );
    assert!(matches!(lex("f'a{x}'").unwrap()[0], Kind::Format(_)));
    assert!(matches!(lex("f\"a\"").unwrap()[0], Kind::Format(_)));
    for (source, words) in [
        ("b1'x'", &["b1"][..]),
        ("b_'x'", &["b_"]),
        ("_b'x'", &["_b"]),
        ("rb'x'", &["rb"]),
        ("ab'x'", &["ab"]),
        ("bb'x'", &["bb"]),
        ("ff\"x\"", &["ff"]),
        ("fbx'x'", &["fbx"]),
        ("u'x'", &["u"]),
        ("r'x'", &["r"]),
        ("b 'x'", &["b"]),
        ("f 'x'", &["f"]),
    ] {
        let mut expected: Vec<Kind> = words.iter().map(|w| word(w)).collect();
        expected.extend([text("x"), Kind::Newline, Kind::Eof]);
        assert_eq!(lex(source).unwrap(), expected, "{source:?}");
    }
    for source in ["b", "f", "bf", "fb", "B", "F", "b1", "x"] {
        assert_eq!(
            lex(source).unwrap(),
            [word(source), Kind::Newline, Kind::Eof],
            "{source:?}"
        );
    }
}

#[test]
fn misspelled_string_prefixes_are_rejected() {
    assert_eq!(
        lex("x = B'a'").unwrap_err(),
        "1:5: byte literals use a lowercase `b` prefix"
    );
    assert_eq!(
        lex("F\"a\"").unwrap_err(),
        "1:1: f-strings use a lowercase `f` prefix"
    );
    for prefix in ["bf", "fb", "Bf", "bF", "fB", "Fb", "BF", "FB"] {
        assert_eq!(
            lex(&format!("{prefix}'a'")).unwrap_err(),
            "1:1: byte f-strings are not supported; build text with an f-string, then convert it to bytes",
            "{prefix:?}"
        );
    }
}

#[test]
fn words_and_numbers_keep_their_spelling_and_columns() {
    let mut lexer = Lexer::new("ab_1 x9  1_000.5e-3u8 é = 0x1F");
    let mut tokens = VecDeque::new();
    let error = lexer.item(&mut tokens).unwrap_err().to_string();
    assert_eq!(error, "1:23: unexpected character `é`");
    let spelled: Vec<_> = tokens
        .iter()
        .map(|token| (token.kind.clone(), token.column))
        .collect();
    assert_eq!(
        spelled,
        [
            (word("ab_1"), 1),
            (word("x9"), 6),
            (Kind::Number("1_000.5e-3u8".into()), 10),
        ]
    );
    let tokens: Vec<_> = {
        let mut lexer = Lexer::new("é = 'é' + 0x1F # é\ny");
        let mut out = VecDeque::new();
        lexer.pos = "é".len();
        lexer.column = 2;
        lexer.line_start = false;
        lexer.item(&mut out).unwrap();
        out.into_iter()
            .map(|t| (t.kind, t.line, t.column))
            .collect()
    };
    assert_eq!(
        tokens,
        [
            (Kind::Symbol("="), 1, 3),
            (text("é"), 1, 5),
            (Kind::Symbol("+"), 1, 9),
            (Kind::Number("31".into()), 1, 11),
            (Kind::Newline, 1, 19),
        ]
    );
}

fn number(s: &str) -> Kind {
    Kind::Number(s.into())
}

/// Hexadecimal, octal, and binary literals reach the parser in decimal with
/// their suffix, so later stages read one integer spelling (issue #563).
#[test]
fn radix_integers_lex_as_decimal() {
    for (source, decimal) in [
        ("0x1F", "31"),
        ("0X1f", "31"),
        ("0x1F1E6", "127462"),
        ("0xFFu8", "255u8"),
        ("0x1E", "30"),
        ("0xffff_ffffu32", "4294967295u32"),
        ("0o17", "15"),
        ("0O17i16", "15i16"),
        ("0b1010", "10"),
        ("0B1_0000_0000u16", "256u16"),
        ("0x0", "0"),
    ] {
        assert_eq!(
            lex(source).unwrap(),
            [number(decimal), Kind::Newline, Kind::Eof],
            "{source}"
        );
    }
    assert_eq!(
        lex("0x10.bit_count()").unwrap()[..3],
        [number("16"), Kind::Symbol("."), word("bit_count")]
    );
}

#[test]
fn malformed_radix_integers_are_rejected() {
    for (source, error) in [
        ("0x", "`0x` needs at least one hexadecimal digit"),
        ("0bu8", "`0b` needs at least one binary digit"),
        ("0b102", "invalid digit `2` in a binary literal"),
        ("0o8", "invalid digit `8` in an octal literal"),
        ("0xFG", "invalid digit `G` in a hexadecimal literal"),
        ("0b1f32", "invalid digit `f` in a binary literal"),
        ("0x1u7", "invalid integer suffix"),
        ("0x_1", "invalid integer separator"),
        ("0x1_", "invalid integer separator"),
        ("0x1__0", "invalid integer separator"),
        ("0x1_u8", "invalid integer separator"),
        (
            "0x1_0000_0000_0000_0000_0000_0000_0000_0000",
            "integer literal out of range",
        ),
    ] {
        assert_eq!(
            lex(source).unwrap_err(),
            format!("1:1: {error}"),
            "{source}"
        );
    }
}

/// `\u{H...}` names one Unicode scalar in string and f-string text, in the
/// spelling `!a` writes (issue #564).
#[test]
fn unicode_escapes_decode_in_strings() {
    for (source, decoded) in [
        (r#""e\u{301}""#, "e\u{301}"),
        (r#"'\u{1F600}\u{0}'"#, "\u{1F600}\0"),
        (r#""\u{10ffff}""#, "\u{10ffff}"),
        (r#""""a\u{200d}b""""#, "a\u{200d}b"),
    ] {
        assert_eq!(
            lex(source).unwrap(),
            [text(decoded), Kind::Newline, Kind::Eof],
            "{source}"
        );
    }
    match &lex(r#"f"\u{e9}{x}""#).unwrap()[0] {
        Kind::Format(parts) => {
            assert_eq!(parts[0], super::fstrings::Part::Literal("\u{e9}".into()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn malformed_unicode_escapes_are_rejected() {
    let shape = "`\\u` needs 1 to 6 hexadecimal digits in braces, as in `\\u{200d}`";
    for (source, error) in [
        (r#"x = "ab\u200d""#, format!("1:8: {shape}")),
        (r#""\u{}""#, format!("1:2: {shape}")),
        (r#""\u{1234567}""#, format!("1:2: {shape}")),
        (r#""\u{12""#, format!("1:2: {shape}")),
        (r#""\u{12x}""#, format!("1:2: {shape}")),
        (r#"f"\u{g}""#, format!("1:3: {shape}")),
        (
            r#""\u{D800}""#,
            "1:2: `\\u{D800}` is a surrogate code point, not a Unicode scalar value".into(),
        ),
        (
            r#"f"\u{dfff}""#,
            "1:3: `\\u{dfff}` is a surrogate code point, not a Unicode scalar value".into(),
        ),
        (
            r#""\u{110000}""#,
            "1:2: `\\u{110000}` is beyond the largest Unicode scalar value, `\\u{10ffff}`".into(),
        ),
        (
            r#"b"\u{41}""#,
            "1:3: unsupported escape \\u in a byte literal; a byte literal holds bytes, not scalars, so write the UTF-8 encoding with `\\xHH` escapes".into(),
        ),
    ] {
        assert_eq!(lex(source).unwrap_err(), error, "{source}");
    }
}

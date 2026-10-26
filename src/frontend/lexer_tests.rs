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
            (Kind::Number("0x1F".into()), 1, 11),
            (Kind::Newline, 1, 19),
        ]
    );
}

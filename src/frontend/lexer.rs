//! Tokens, source-file identities and the streaming lexer. The lexer reads
//! no lowering state; a lexical error has no path until the module loader
//! adds it.

use super::*;

/// Every symbol the lexer accepts, so a symbol token needs no allocation.
pub(super) const SYMBOLS: [&str; 43] = [
    "@", "(", ")", "[", "]", "{", "}", ".", ":", ",", "+", "-", "*", "%", "/", "=", "!", "<", ">",
    "&", "|", "^", "~", "?", "->", "==", "!=", "<=", ">=", "//", "<<", ">>", "+=", "-=", "*=",
    "/=", "//=", "%=", "&=", "|=", "^=", "<<=", ">>=",
];

/// The bytes that continue a word: ASCII letters, digits and `_`.
const WORD_BYTES: [bool; 256] = {
    let mut table = [false; 256];
    let mut c = 0;
    while c < 256 {
        table[c] = (c as u8).is_ascii_alphanumeric() || c == b'_' as usize;
        c += 1;
    }
    table
};

/// The longest listed symbol starting with `c`, given the two bytes after it.
/// Matching the bytes directly avoids searching `SYMBOLS` (issue #431).
pub(super) fn symbol(c: u8, next: Option<&u8>, after: Option<&u8>) -> &'static str {
    match (c, next) {
        (b'-', Some(b'>')) => "->",
        (b'=', Some(b'=')) => "==",
        (b'!', Some(b'=')) => "!=",
        (b'<', Some(b'=')) => "<=",
        (b'>', Some(b'=')) => ">=",
        (b'<', Some(b'<')) if after == Some(&b'=') => "<<=",
        (b'>', Some(b'>')) if after == Some(&b'=') => ">>=",
        (b'<', Some(b'<')) => "<<",
        (b'>', Some(b'>')) => ">>",
        (b'/', Some(b'/')) if after == Some(&b'=') => "//=",
        (b'/', Some(b'/')) => "//",
        (b'+', Some(b'=')) => "+=",
        (b'-', Some(b'=')) => "-=",
        (b'*', Some(b'=')) => "*=",
        (b'/', Some(b'=')) => "/=",
        (b'%', Some(b'=')) => "%=",
        (b'&', Some(b'=')) => "&=",
        (b'|', Some(b'=')) => "|=",
        (b'^', Some(b'=')) => "^=",
        (b'(', _) => "(",
        (b')', _) => ")",
        (b'[', _) => "[",
        (b']', _) => "]",
        (b'{', _) => "{",
        (b'}', _) => "}",
        (b'.', _) => ".",
        (b':', _) => ":",
        (b',', _) => ",",
        (b'+', _) => "+",
        (b'-', _) => "-",
        (b'*', _) => "*",
        (b'%', _) => "%",
        (b'/', _) => "/",
        (b'=', _) => "=",
        (b'!', _) => "!",
        (b'<', _) => "<",
        (b'>', _) => ">",
        (b'&', _) => "&",
        (b'|', _) => "|",
        (b'^', _) => "^",
        (b'~', _) => "~",
        (b'?', _) => "?",
        (b'@', _) => "@",
        _ => unreachable!("the lexer calls this only for symbol characters"),
    }
}

/// The string prefix spelled by a word directly followed by a quote. Any
/// other word before a quote stays a word.
fn literal_prefix(word: &[u8]) -> Option<std::result::Result<Prefix, &'static str>> {
    Some(match word {
        b"b" => Ok(Prefix::Bytes),
        b"B" => Err("byte literals use a lowercase `b` prefix"),
        b"f" => Ok(Prefix::Format),
        b"F" => Err("f-strings use a lowercase `f` prefix"),
        b"bf" | b"fb" | b"Bf" | b"bF" | b"fB" | b"Fb" | b"BF" | b"FB" => Err(
            "byte f-strings are not supported; build text with an f-string, then convert it to bytes",
        ),
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    /// Word, number and text tokens share their text: the parser clones a
    /// token for every position it records.
    Word(Arc<str>),
    Number(Arc<str>),
    Text(Arc<str>),
    /// An `f"..."` literal's text and fields; see `fstrings`.
    Format(Arc<[fstrings::Part]>),
    Bytes(Arc<[u8]>),
    Symbol(&'static str),
    Newline,
    Indent,
    Dedent,
    Eof,
}

/// An error at a position in source that has no token there. The lexer does
/// not know its module's path; the module loader adds it.
pub(super) fn lexical(line: u32, column: u32, message: &str) -> Box<dyn Error> {
    let at = Position {
        file: None,
        line,
        column,
    };
    Box::new(Diagnostic::new(Some(at), message))
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Token {
    pub(super) kind: Kind,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) file: Option<FileId>,
}

/// A module path interned for the life of the process. Every token names
/// its file, and a four-byte copy costs no reference count where an
/// `Arc<str>` cost two atomic updates per token (issue #422).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct FileId(pub(super) std::num::NonZeroU32);

#[derive(Default)]
pub(super) struct Files {
    pub(super) ids: HashMap<Arc<str>, FileId>,
    pub(super) paths: Vec<Arc<str>>,
}

static FILES: std::sync::LazyLock<std::sync::RwLock<Files>> =
    std::sync::LazyLock::new(Default::default);

impl FileId {
    pub(super) fn intern(path: &Arc<str>) -> FileId {
        if let Some(&id) = FILES.read().unwrap().ids.get(path) {
            return id;
        }
        let mut files = FILES.write().unwrap();
        if let Some(&id) = files.ids.get(path) {
            return id;
        }
        files.paths.push(path.clone());
        let id = FileId(
            std::num::NonZeroU32::new(u32::try_from(files.paths.len()).unwrap())
                .expect("a file number starts at one"),
        );
        files.ids.insert(path.clone(), id);
        id
    }

    pub(super) fn path(self) -> Arc<str> {
        FILES.read().unwrap().paths[self.0.get() as usize - 1].clone()
    }
}

impl Token {
    /// The path of the module the token is in, when it came from a file.
    pub(super) fn source(&self) -> Option<Arc<str>> {
        self.file.map(FileId::path)
    }

    pub(super) fn is(&self, text: &str) -> bool {
        self.text() == Some(text)
    }

    /// The spelling of a word or symbol.
    pub(super) fn text(&self) -> Option<&str> {
        match &self.kind {
            Kind::Word(s) => Some(s),
            Kind::Symbol(s) => Some(s),
            _ => None,
        }
    }

    pub(super) fn error(&self, message: impl std::fmt::Display) -> Box<dyn Error> {
        Box::new(Diagnostic::new(Some(self.position()), message.to_string()))
    }

    /// An error whose cause is a requirement on a value's type; see
    /// `Diagnostic::demand`.
    pub(super) fn demand_error(
        &self,
        message: impl std::fmt::Display,
        demand: crate::ownership::Demand,
    ) -> Box<dyn Error> {
        Box::new(Diagnostic {
            demand: Some(Box::new(demand)),
            ..Diagnostic::new(Some(self.position()), message.to_string())
        })
    }

    pub(super) fn position(&self) -> Position {
        Position {
            file: self.source(),
            line: self.line,
            column: self.column,
        }
    }

    pub(super) fn location(&self) -> String {
        self.position().to_string()
    }
}

/// Lexes a module one top-level item at a time. The parser releases an item's
/// tokens once it has the item's tree, so a module's tokens never all exist
/// beside its syntax tree (issue #54).
pub(super) struct Lexer<'a> {
    pub(super) source: &'a str,
    /// Byte offset of the next character.
    pub(super) pos: usize,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) line_start: bool,
    pub(super) indents: Vec<usize>,
    /// Open brackets with their positions, so an unclosed one is reported where it opened.
    pub(super) delimiters: Vec<(u8, Token)>,
    /// Whether the last token leaves a logical line for a line break to end.
    pub(super) open: bool,
    /// No tokens remain: the module ended or has a lexical error.
    pub(super) done: bool,
}

impl<'a> Lexer<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Lexer {
            source,
            pos: 0,
            line: 1,
            column: 1,
            line_start: true,
            indents: vec![0],
            delimiters: Vec::new(),
            open: false,
            done: false,
        }
    }

    /// Appends the next top-level item's tokens: everything up to the next
    /// line that starts at the left margin outside brackets, or to the end of
    /// the module, where `Eof` is the last token.
    pub(super) fn item(&mut self, out: &mut VecDeque<Token>) -> Result<()> {
        let lexed = self.lex_item(out);
        self.done |= lexed.is_err();
        lexed
    }

    /// Lexes the rest of the module for its errors alone.
    pub(super) fn rest(&mut self) -> Result<()> {
        let mut tokens = VecDeque::new();
        while !self.done {
            tokens.clear();
            self.item(&mut tokens)?;
        }
        Ok(())
    }

    /// Moves the column past `width` characters. Positions are 32-bit to
    /// keep tokens small; a column past `u32::MAX` stays there.
    fn skip_columns(&mut self, width: usize) {
        self.column = self
            .column
            .saturating_add(u32::try_from(width).unwrap_or(u32::MAX));
    }

    pub(super) fn push(&mut self, out: &mut VecDeque<Token>, kind: Kind, line: u32, column: u32) {
        self.open = !matches!(kind, Kind::Newline | Kind::Dedent);
        out.push_back(Token {
            kind,
            line,
            column,
            file: None,
        });
    }

    /// The body of `b"..."` from its opening quote. Only ASCII characters and
    /// escapes may appear, so each byte has exactly one spelling.
    fn byte_literal(&mut self, start: &Token) -> Result<Vec<u8>> {
        let source = self.source;
        let bytes = source.as_bytes();
        let quote = bytes[self.pos];
        let closes = |pos: usize| bytes.get(pos) == Some(&quote);
        let triple = closes(self.pos + 1) && closes(self.pos + 2);
        let width = if triple { 3 } else { 1 };
        self.pos += width;
        self.skip_columns(width);
        let mut value = Vec::new();
        loop {
            let here = Token {
                kind: Kind::Eof,
                line: self.line,
                column: self.column,
                file: None,
            };
            let ch = source[self.pos..]
                .chars()
                .next()
                .ok_or_else(|| start.error("unterminated byte literal"))?;
            if ch == quote as char && (!triple || (closes(self.pos + 1) && closes(self.pos + 2))) {
                self.pos += width;
                self.skip_columns(width);
                return Ok(value);
            }
            if ch == '\n' && !triple {
                return Err(start.error("newline in a quoted byte literal"));
            }
            if !ch.is_ascii() {
                return Err(here.error(format!(
                    "a byte literal holds only ASCII characters; write `{ch}` with `\\x` escapes or convert text with `std.bytes.from_text`"
                )));
            }
            self.pos += 1;
            self.column += 1;
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            }
            if ch != '\\' {
                value.push(ch as u8);
                continue;
            }
            if self.pos == bytes.len() {
                return Err(start.error("unterminated escape"));
            }
            let (byte, width) = byte_escape(&source[self.pos..]).map_err(|m| here.error(m))?;
            value.push(byte);
            self.pos += width;
            self.skip_columns(width);
        }
    }

    fn lex_item(&mut self, out: &mut VecDeque<Token>) -> Result<()> {
        // Every character with a meaning outside strings and comments is
        // ASCII, and no byte of a longer UTF-8 sequence is, so the bytes are
        // compared directly. Columns count characters.
        let source = self.source;
        let bytes = source.as_bytes();
        let before = out.len();
        while self.pos < bytes.len() {
            if self.line_start {
                let mut spaces = 0;
                while bytes.get(self.pos) == Some(&b' ') {
                    spaces += 1;
                    self.pos += 1;
                    self.column += 1;
                }
                let (line, column) = (self.line, self.column);
                if bytes.get(self.pos) == Some(&b'\t') {
                    return Err(lexical(
                        line,
                        column,
                        "use spaces, not tabs, for indentation",
                    ));
                }
                let code = self.delimiters.is_empty()
                    && !matches!(bytes.get(self.pos), None | Some(b'\n' | b'\r' | b'#'));
                if code {
                    let last = *self.indents.last().unwrap();
                    if spaces > last {
                        self.indents.push(spaces);
                        self.push(out, Kind::Indent, line, column);
                    } else {
                        while spaces < *self.indents.last().unwrap() {
                            self.indents.pop();
                            self.push(out, Kind::Dedent, line, column);
                        }
                        if spaces != *self.indents.last().unwrap() {
                            return Err(lexical(
                                line,
                                column,
                                "indentation does not match an outer block",
                            ));
                        }
                    }
                }
                self.line_start = false;
                if self.pos == bytes.len() {
                    break;
                }
                if code && spaces == 0 && out.len() > before {
                    return Ok(());
                }
            }
            let (line, column) = (self.line, self.column);
            let start = Token {
                kind: Kind::Eof,
                line,
                column,
                file: None,
            };
            let c = bytes[self.pos];
            match c {
                b' ' | b'\r' => {
                    self.pos += 1;
                    self.column += 1;
                }
                b'\t' => return Err(start.error("tabs are not supported; use spaces")),
                b'\n' => {
                    if self.delimiters.is_empty() && self.open {
                        self.push(out, Kind::Newline, line, column);
                    }
                    self.pos += 1;
                    self.line += 1;
                    self.column = 1;
                    self.line_start = true;
                }
                b'#' => {
                    let end = bytes[self.pos..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(bytes.len(), |length| self.pos + length);
                    self.skip_columns(source[self.pos..end].chars().count());
                    self.pos = end;
                }
                b'\'' | b'"' => {
                    let quote = c as char;
                    let closes = |pos: usize| bytes.get(pos) == Some(&c);
                    let triple = closes(self.pos + 1) && closes(self.pos + 2);
                    let width = if triple { 3 } else { 1 };
                    self.pos += width;
                    self.skip_columns(width);
                    let mut text = String::new();
                    loop {
                        let ch = source[self.pos..]
                            .chars()
                            .next()
                            .ok_or_else(|| start.error("unterminated string"))?;
                        if ch == quote
                            && (!triple || (closes(self.pos + 1) && closes(self.pos + 2)))
                        {
                            self.pos += width;
                            self.skip_columns(width);
                            break;
                        }
                        if ch == '\n' && !triple {
                            return Err(start.error("newline in a quoted string"));
                        }
                        let here = Token {
                            kind: Kind::Eof,
                            line: self.line,
                            column: self.column,
                            file: None,
                        };
                        self.pos += ch.len_utf8();
                        self.column += 1;
                        if ch == '\\' {
                            let escaped = source[self.pos..]
                                .chars()
                                .next()
                                .ok_or_else(|| start.error("unterminated escape"))?;
                            self.pos += escaped.len_utf8();
                            self.column += 1;
                            if escaped == 'u' {
                                let (scalar, length) = unicode_escape(&source[self.pos..])
                                    .map_err(|e| here.error(e))?;
                                // The escape is ASCII, so bytes and columns agree.
                                self.pos += length;
                                self.skip_columns(length);
                                text.push(scalar);
                                continue;
                            }
                            text.push(escape(escaped).ok_or_else(|| {
                                here.error(format!("unsupported escape \\{escaped}"))
                            })?);
                        } else {
                            if ch == '\n' {
                                self.line += 1;
                                self.column = 1;
                            }
                            text.push(ch);
                        }
                    }
                    self.push(out, Kind::Text(text.into()), line, column);
                }
                c if c.is_ascii_alphabetic() || c == b'_' => {
                    let begin = self.pos;
                    self.pos += bytes[begin..]
                        .iter()
                        .position(|&c| !WORD_BYTES[c as usize])
                        .unwrap_or(bytes.len() - begin);
                    self.skip_columns(self.pos - begin);
                    let prefix = if matches!(bytes.get(self.pos), Some(b'\'' | b'"')) {
                        literal_prefix(&bytes[begin..self.pos])
                    } else {
                        None
                    };
                    match prefix {
                        None => self.push(
                            out,
                            Kind::Word(source[begin..self.pos].into()),
                            line,
                            column,
                        ),
                        Some(Err(message)) => return Err(start.error(message)),
                        Some(Ok(Prefix::Bytes)) => {
                            let value = self.byte_literal(&start)?;
                            self.push(out, Kind::Bytes(value.into()), line, column);
                        }
                        Some(Ok(Prefix::Format)) => {
                            let kind = self.format_string(&start)?;
                            self.push(out, kind, line, column);
                        }
                    }
                }
                b'0' if matches!(
                    bytes.get(self.pos + 1),
                    Some(b'x' | b'X' | b'o' | b'O' | b'b' | b'B')
                ) =>
                {
                    let begin = self.pos;
                    let end = begin
                        + 2
                        + bytes[begin + 2..]
                            .iter()
                            .position(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
                            .unwrap_or(bytes.len() - begin - 2);
                    let decimal = radix_integer(&source[begin..end]).map_err(|e| start.error(e))?;
                    self.pos = end;
                    self.skip_columns(end - begin);
                    self.push(out, Kind::Number(decimal.into()), line, column);
                }
                c if c.is_ascii_digit()
                    || (c == b'.' && bytes.get(self.pos + 1).is_some_and(u8::is_ascii_digit)) =>
                {
                    let begin = self.pos;
                    let mut pos = self.pos;
                    let digits = |pos: &mut usize| {
                        while bytes
                            .get(*pos)
                            .is_some_and(|c| c.is_ascii_digit() || *c == b'_')
                        {
                            *pos += 1;
                        }
                    };
                    digits(&mut pos);
                    if bytes.get(pos) == Some(&b'.') {
                        pos += 1;
                        digits(&mut pos);
                    }
                    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
                        pos += 1;
                        if matches!(bytes.get(pos), Some(b'+' | b'-')) {
                            pos += 1;
                        }
                        digits(&mut pos);
                    }
                    // Keep a suffix (including malformed suffixes) in the same token.
                    while bytes
                        .get(pos)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    {
                        pos += 1;
                    }
                    self.pos = pos;
                    self.skip_columns(pos - begin);
                    self.push(out, Kind::Number(source[begin..pos].into()), line, column);
                }
                b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'.' | b':' | b',' | b'+' | b'-'
                | b'*' | b'%' | b'/' | b'=' | b'!' | b'<' | b'>' | b'&' | b'|' | b'^' | b'~'
                | b'?' | b'@' => {
                    let symbol = symbol(c, bytes.get(self.pos + 1), bytes.get(self.pos + 2));
                    debug_assert!(SYMBOLS.contains(&symbol));
                    if matches!(c, b'(' | b'[' | b'{') {
                        self.delimiters.push((c, start.clone()));
                    }
                    if matches!(c, b')' | b']' | b'}') {
                        let expected = match c {
                            b')' => b'(',
                            b']' => b'[',
                            _ => b'{',
                        };
                        if self.delimiters.pop().map(|(open, _)| open) != Some(expected) {
                            return Err(
                                start.error(format!("unmatched or mismatched `{}`", c as char))
                            );
                        }
                    }
                    self.pos += symbol.len();
                    self.skip_columns(symbol.len());
                    self.push(out, Kind::Symbol(symbol), line, column);
                }
                _ => {
                    let c = source[self.pos..].chars().next().unwrap();
                    return Err(start.error(format!("unexpected character `{c}`")));
                }
            }
        }
        let (line, column) = (self.line, self.column);
        if let Some((open, at)) = self.delimiters.last() {
            return Err(at.error(format!("unclosed `{}`", *open as char)));
        }
        if self.open {
            self.push(out, Kind::Newline, line, column);
        }
        for _ in 1..self.indents.len() {
            self.push(out, Kind::Dedent, line, column);
        }
        self.push(out, Kind::Eof, line, column);
        self.done = true;
        Ok(())
    }
}

/// A string prefix. Each has exactly one spelling, matched in
/// `literal_prefix`.
#[derive(Clone, Copy, Debug)]
pub(super) enum Prefix {
    Bytes,
    Format,
}

/// Decodes the escape after a backslash in byte text: a shared escape or
/// `\xHH`. Returns the byte and the source bytes consumed after the backslash.
fn byte_escape(rest: &str) -> std::result::Result<(u8, usize), String> {
    let escaped = rest.chars().next().expect("caller checked for input");
    if escaped == 'x' {
        let digits = rest
            .get(1..3)
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("`\\x` needs exactly two hexadecimal digits")?;
        return Ok((u8::from_str_radix(digits, 16).expect("hex digits"), 3));
    }
    match escape(escaped) {
        Some(byte) => Ok((byte as u8, 1)),
        None if escaped == 'u' => Err(
            "unsupported escape \\u in a byte literal; a byte literal holds bytes, not scalars, so write the UTF-8 encoding with `\\xHH` escapes".into(),
        ),
        None => Err(format!("unsupported escape \\{escaped} in a byte literal")),
    }
}

/// Decodes a `\u{H...}` escape in string text. `rest` follows the `\u`;
/// returns the scalar and the source bytes consumed. The spelling is the one
/// the `!a` conversion writes, so its output can be pasted back into source.
pub(super) fn unicode_escape(rest: &str) -> std::result::Result<(char, usize), String> {
    let shape = || "`\\u` needs 1 to 6 hexadecimal digits in braces, as in `\\u{200d}`".to_string();
    let digits = rest
        .strip_prefix('{')
        .map(|inner| {
            &inner[..inner
                .find(|c: char| !c.is_ascii_hexdigit())
                .unwrap_or(inner.len())]
        })
        .filter(|digits| (1..=6).contains(&digits.len()))
        .ok_or_else(shape)?;
    if rest.as_bytes().get(digits.len() + 1) != Some(&b'}') {
        return Err(shape());
    }
    let value = u32::from_str_radix(digits, 16).expect("at most six hexadecimal digits");
    let scalar = char::from_u32(value).ok_or_else(|| {
        if value > 0x10ffff {
            format!("`\\u{{{digits}}}` is beyond the largest Unicode scalar value, `\\u{{10ffff}}`")
        } else {
            format!("`\\u{{{digits}}}` is a surrogate code point, not a Unicode scalar value")
        }
    })?;
    Ok((scalar, digits.len() + 2))
}

/// Rewrites a `0x`, `0o`, or `0b` integer literal in decimal with the same
/// suffix, so every later stage reads one integer spelling. Range checks
/// against the suffix or context type happen there, as for decimal literals.
fn radix_integer(text: &str) -> std::result::Result<String, String> {
    let (radix, article, name) = match text.as_bytes()[1] {
        b'x' | b'X' => (16, "a", "hexadecimal"),
        b'o' | b'O' => (8, "an", "octal"),
        _ => (2, "a", "binary"),
    };
    let body = &text[2..];
    let split = body
        .find(|c: char| c != '_' && !c.is_digit(radix))
        .unwrap_or(body.len());
    let (digits, suffix) = body.split_at(split);
    // A suffix names an integer type, so anything else after the digits is a
    // misplaced digit: `0b102`, `0xG`, or a float suffix such as `0b1f32`.
    if !suffix.is_empty() && !suffix.starts_with(['i', 'u']) {
        let bad = suffix.chars().next().unwrap();
        return Err(format!("invalid digit `{bad}` in {article} {name} literal"));
    }
    if !matches!(
        suffix,
        "" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64"
    ) {
        return Err("invalid integer suffix".into());
    }
    if digits.is_empty() {
        return Err(format!("`{}` needs at least one {name} digit", &text[..2]));
    }
    if digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return Err("invalid integer separator".into());
    }
    let value = digits
        .bytes()
        .filter(|&c| c != b'_')
        .try_fold(0u128, |value, c| {
            let digit = (c as char).to_digit(radix).expect("checked digit");
            value.checked_mul(radix.into())?.checked_add(digit.into())
        })
        .ok_or("integer literal out of range")?;
    Ok(format!("{value}{suffix}"))
}

/// The escapes string and byte literals share; byte literals add `\xHH`.
pub(super) fn escape(c: char) -> Option<char> {
    Some(match c {
        '0' => '\0',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '\\' => '\\',
        '\'' => '\'',
        '"' => '"',
        _ => return None,
    })
}

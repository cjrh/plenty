//! The TOML subset that `plenty.toml` uses, parsed strictly.
//!
//! Manifests need tables, arrays of tables, inline tables, strings, integers,
//! booleans, and arrays. A complete TOML implementation would add the first
//! third-party parser to the compiler's dependency graph (with serde) for
//! features a manifest never needs, such as dates, floats, and dotted keys. An
//! unsupported construct is an error at its line, never a silent
//! reinterpretation, so manifests stay valid TOML 1.0.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    String(String),
    Integer(i64),
    Bool(bool),
    Array(Vec<Value>),
    Table(Table),
}

/// Keys in source order are not needed: manifests are read, not edited.
pub(crate) type Table = BTreeMap<String, Entry>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) value: Value,
    /// 1-based line of the key, for diagnostics.
    pub(crate) line: usize,
    /// Whether `[[...]]` headers created this array. TOML lets headers extend
    /// and descend into only these arrays, not statically defined ones.
    pub(crate) table_array: bool,
    /// For such an array, the line of each element's header.
    pub(crate) header_lines: Vec<usize>,
}

impl Value {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Value::String(_) => "a string",
            Value::Integer(_) => "an integer",
            Value::Bool(_) => "a boolean",
            Value::Array(_) => "an array",
            Value::Table(_) => "a table",
        }
    }
}

/// Bounds keep a hostile manifest from exhausting memory or the stack.
const MAX_INPUT: usize = 1 << 20;
const MAX_DEPTH: usize = 16;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    name: &'a str,
}

type Result<T> = std::result::Result<T, String>;

pub(crate) fn parse(text: &str, name: &str) -> Result<Table> {
    if text.len() > MAX_INPUT {
        return Err(format!("{name}: larger than {MAX_INPUT} bytes"));
    }
    let mut parser = Parser {
        bytes: text.as_bytes(),
        pos: 0,
        line: 1,
        name,
    };
    parser.document()
}

impl Parser<'_> {
    fn error<T>(&self, message: impl std::fmt::Display) -> Result<T> {
        self.error_at(self.line, message)
    }

    /// For errors found after the line's end has been consumed.
    fn error_at<T>(&self, line: usize, message: impl std::fmt::Display) -> Result<T> {
        Err(format!("{}:{line}: {message}", self.name))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn blank(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    /// Skip spaces and a comment, then require the end of the line.
    fn end_of_line(&mut self) -> Result<()> {
        self.blank();
        if self.peek() == Some(b'#') {
            while !matches!(self.peek(), None | Some(b'\n')) {
                self.pos += 1;
            }
        }
        match self.peek() {
            None => Ok(()),
            Some(b'\n') => {
                self.pos += 1;
                self.line += 1;
                Ok(())
            }
            Some(b'\r') if self.bytes.get(self.pos + 1) == Some(&b'\n') => {
                self.pos += 2;
                self.line += 1;
                Ok(())
            }
            Some(_) => self.error("expected the end of the line"),
        }
    }

    /// Whitespace, newlines and comments, as allowed inside arrays.
    fn space(&mut self) {
        loop {
            self.blank();
            match self.peek() {
                Some(b'#') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.pos += 1;
                    }
                }
                Some(b'\n') => {
                    self.pos += 1;
                    self.line += 1;
                }
                Some(b'\r') if self.bytes.get(self.pos + 1) == Some(&b'\n') => {
                    self.pos += 2;
                    self.line += 1;
                }
                _ => return,
            }
        }
    }

    fn document(&mut self) -> Result<Table> {
        let mut root = Table::new();
        // The path of the current `[table]`, and whether it is the newest
        // element of an array of tables.
        let mut current: Vec<String> = vec![];
        let mut array_element = false;
        // Headers of explicitly defined tables. A set keeps the duplicate
        // check logarithmic, so a hostile manifest within `MAX_INPUT` cannot
        // force quadratic work.
        let mut defined: BTreeSet<Vec<String>> = BTreeSet::new();
        loop {
            self.space();
            match self.peek() {
                None => return Ok(root),
                Some(b'[') => {
                    let line = self.line;
                    self.pos += 1;
                    let array = self.peek() == Some(b'[');
                    if array {
                        self.pos += 1;
                    }
                    self.blank();
                    let path = self.key_path()?;
                    self.blank();
                    for _ in 0..if array { 2 } else { 1 } {
                        if self.peek() != Some(b']') {
                            return self.error("expected `]` after the table name");
                        }
                        self.pos += 1;
                    }
                    self.end_of_line()?;
                    if array {
                        let (parent, last) = path.split_at(path.len() - 1);
                        let table = self.table_at(&mut root, parent, line)?;
                        let entry = table.entry(last[0].clone()).or_insert(Entry {
                            value: Value::Array(vec![]),
                            line,
                            table_array: true,
                            header_lines: vec![],
                        });
                        match &mut entry.value {
                            Value::Array(items) if entry.table_array => {
                                items.push(Value::Table(Table::new()));
                                entry.header_lines.push(line);
                                // Headers below `path` named tables of the
                                // previous element; the new element may define
                                // them again. Sorted order keeps them adjacent.
                                let stale: Vec<Vec<String>> = defined
                                    .range::<Vec<String>, _>(&path..)
                                    .take_while(|p| p.starts_with(&path))
                                    .cloned()
                                    .collect();
                                for p in &stale {
                                    defined.remove(p);
                                }
                            }
                            _ => {
                                return self.error_at(
                                    line,
                                    format!("`{}` is already defined as a value", path.join(".")),
                                );
                            }
                        }
                    } else {
                        if !defined.insert(path.clone()) {
                            return self.error_at(
                                line,
                                format!("table `{}` is defined twice", path.join(".")),
                            );
                        }
                        // `table_at` descends into an array's newest element,
                        // which TOML allows only for intermediate segments:
                        // `[a]` after `[[a]]` must not reopen that element.
                        let (parent, last) = path.split_at(path.len() - 1);
                        let parent = self.table_at(&mut root, parent, line)?;
                        if let Some(Entry {
                            value: Value::Array(_),
                            ..
                        }) = parent.get(&last[0])
                        {
                            return self.error_at(
                                line,
                                format!("`{}` is already defined as an array", path.join(".")),
                            );
                        }
                        self.table_at(&mut root, &path, line)?;
                    }
                    current = path;
                    array_element = array;
                }
                Some(_) => {
                    let line = self.line;
                    let key = self.key()?;
                    self.blank();
                    if self.peek() == Some(b'.') {
                        return self.error("dotted keys are not supported in manifests");
                    }
                    if self.peek() != Some(b'=') {
                        return self.error(format!("expected `=` after `{key}`"));
                    }
                    self.pos += 1;
                    self.blank();
                    let value = self.value(0)?;
                    self.end_of_line()?;
                    let table = if array_element {
                        let (parent, last) = current.split_at(current.len() - 1);
                        let table = self.table_at(&mut root, parent, line)?;
                        match &mut table.get_mut(&last[0]).unwrap().value {
                            Value::Array(items) => match items.last_mut() {
                                Some(Value::Table(table)) => table,
                                _ => unreachable!("array of tables holds tables"),
                            },
                            _ => unreachable!("array of tables"),
                        }
                    } else {
                        self.table_at(&mut root, &current, line)?
                    };
                    if table.contains_key(&key) {
                        return self.error_at(line, format!("key `{key}` is defined twice"));
                    }
                    table.insert(
                        key,
                        Entry {
                            value,
                            line,
                            table_array: false,
                            header_lines: vec![],
                        },
                    );
                }
            }
        }
    }

    /// The table at `path`, creating implicit parents. Descending through an
    /// array of tables selects its newest element, as TOML specifies.
    fn table_at<'t>(
        &self,
        root: &'t mut Table,
        path: &[String],
        line: usize,
    ) -> Result<&'t mut Table> {
        let mut table = root;
        for (i, part) in path.iter().enumerate() {
            let entry = table.entry(part.clone()).or_insert(Entry {
                value: Value::Table(Table::new()),
                line,
                table_array: false,
                header_lines: vec![],
            });
            table = match &mut entry.value {
                Value::Table(table) => table,
                Value::Array(items) if entry.table_array => match items.last_mut() {
                    Some(Value::Table(table)) => table,
                    _ => unreachable!("array of tables holds tables"),
                },
                _ => {
                    return Err(format!(
                        "{}:{line}: `{}` is not a table",
                        self.name,
                        path[..=i].join(".")
                    ));
                }
            };
        }
        Ok(table)
    }

    fn key_path(&mut self) -> Result<Vec<String>> {
        let mut path = vec![self.key()?];
        loop {
            self.blank();
            if self.peek() != Some(b'.') {
                return Ok(path);
            }
            self.pos += 1;
            self.blank();
            path.push(self.key()?);
        }
    }

    fn key(&mut self) -> Result<String> {
        if self.peek() == Some(b'"') {
            return self.basic_string();
        }
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            self.pos += 1;
        }
        if start == self.pos {
            return self.error("expected a key");
        }
        Ok(String::from_utf8(self.bytes[start..self.pos].to_vec()).unwrap())
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return self.error("values are nested too deeply");
        }
        match self.peek() {
            Some(b'"') => {
                if self.bytes[self.pos..].starts_with(b"\"\"\"") {
                    return self.error("multi-line strings are not supported in manifests");
                }
                self.basic_string().map(Value::String)
            }
            Some(b'\'') => {
                if self.bytes[self.pos..].starts_with(b"'''") {
                    return self.error("multi-line strings are not supported in manifests");
                }
                self.pos += 1;
                let start = self.pos;
                while !matches!(self.peek(), None | Some(b'\'' | b'\n')) {
                    self.pos += 1;
                }
                if self.peek() != Some(b'\'') {
                    return self.error("unterminated string");
                }
                let text = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| format!("{}:{}: invalid UTF-8", self.name, self.line))?
                    .to_owned();
                self.pos += 1;
                Ok(Value::String(text))
            }
            Some(b'[') => {
                self.pos += 1;
                let mut items = vec![];
                loop {
                    self.space();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        return Ok(Value::Array(items));
                    }
                    items.push(self.value(depth + 1)?);
                    self.space();
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {}
                        _ => return self.error("expected `,` or `]` in an array"),
                    }
                }
            }
            Some(b'{') => {
                self.pos += 1;
                let mut table = Table::new();
                self.blank();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(Value::Table(table));
                }
                loop {
                    self.blank();
                    let line = self.line;
                    let key = self.key()?;
                    self.blank();
                    if self.peek() != Some(b'=') {
                        return self.error(format!("expected `=` after `{key}`"));
                    }
                    self.pos += 1;
                    self.blank();
                    let value = self.value(depth + 1)?;
                    if table.contains_key(&key) {
                        return self.error_at(line, format!("key `{key}` is defined twice"));
                    }
                    table.insert(
                        key,
                        Entry {
                            value,
                            line,
                            table_array: false,
                            header_lines: vec![],
                        },
                    );
                    self.blank();
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            return Ok(Value::Table(table));
                        }
                        _ => return self.error("expected `,` or `}` in an inline table"),
                    }
                }
            }
            Some(b't') if self.bytes[self.pos..].starts_with(b"true") => {
                self.pos += 4;
                Ok(Value::Bool(true))
            }
            Some(b'f') if self.bytes[self.pos..].starts_with(b"false") => {
                self.pos += 5;
                Ok(Value::Bool(false))
            }
            Some(c) if c.is_ascii_digit() || c == b'-' || c == b'+' => {
                let start = self.pos;
                self.pos += 1;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'_') {
                    self.pos += 1;
                }
                let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
                let digits = text.trim_start_matches(['+', '-']);
                let valid = !digits.is_empty()
                    && !digits.starts_with('_')
                    && !digits.ends_with('_')
                    && !digits.contains("__")
                    && (digits == "0" || !digits.starts_with('0'));
                if !valid || matches!(self.peek(), Some(b'.' | b'e' | b'E' | b':')) {
                    return self.error("only decimal integers are supported in manifests");
                }
                text.replace('_', "")
                    .parse()
                    .map(Value::Integer)
                    .or_else(|_| self.error("integer out of range"))
            }
            _ => self.error("expected a value"),
        }
    }

    fn basic_string(&mut self) -> Result<String> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => return self.error("unterminated string"),
                Some(b'"') => {
                    self.pos += 1;
                    return String::from_utf8(out)
                        .map_err(|_| format!("{}:{}: invalid UTF-8", self.name, self.line));
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let escaped = match self.peek() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        Some(b'r') => '\r',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'u') => {
                            let hex = self
                                .bytes
                                .get(self.pos + 1..self.pos + 5)
                                // `from_str_radix` alone would accept a `+` sign.
                                .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .and_then(char::from_u32);
                            let Some(c) = hex else {
                                return self.error("invalid \\u escape");
                            };
                            self.pos += 4;
                            c
                        }
                        _ => return self.error("invalid escape in string"),
                    };
                    self.pos += 1;
                    let mut buffer = [0; 4];
                    out.extend_from_slice(escaped.encode_utf8(&mut buffer).as_bytes());
                }
                Some(c) if c < 0x20 && c != b'\t' => {
                    return self.error("control character in string");
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }
}

/// Quote `text` as a TOML basic string.
pub(crate) fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(table: &'a Table, key: &str) -> &'a Value {
        &table[key].value
    }

    #[test]
    fn tables_arrays_and_inline_tables() {
        let text = "manifest-version = 1 # format\n\
            [package]\nname = \"player\"\nauthors = [\n  \"A\", # first\n  'B',\n]\n\
            [dependencies]\nutil = { path = \"../util\" }\n\
            [dependencies.audio]\npath = '../audio'\n\
            [[bin]]\nname = \"one\"\n[[bin]]\nname = \"two\"\n";
        let doc = parse(text, "plenty.toml").unwrap();
        assert_eq!(get(&doc, "manifest-version"), &Value::Integer(1));
        let Value::Table(package) = get(&doc, "package") else {
            panic!()
        };
        assert_eq!(
            get(package, "authors"),
            &Value::Array(vec![Value::String("A".into()), Value::String("B".into())])
        );
        let Value::Table(deps) = get(&doc, "dependencies") else {
            panic!()
        };
        assert_eq!(deps.len(), 2);
        let Value::Array(bins) = get(&doc, "bin") else {
            panic!()
        };
        assert_eq!(bins.len(), 2);
    }

    #[test]
    fn subtable_header_extends_newest_array_element() {
        let text = "[[a]]\nx = 1\n[[a]]\nx = 2\n[a.b]\ny = 3\n";
        let doc = parse(text, "plenty.toml").unwrap();
        let Value::Array(items) = get(&doc, "a") else {
            panic!()
        };
        let Value::Table(last) = &items[1] else {
            panic!()
        };
        let Value::Table(b) = get(last, "b") else {
            panic!()
        };
        assert_eq!(get(b, "y"), &Value::Integer(3));
        let Value::Table(first) = &items[0] else {
            panic!()
        };
        assert!(!first.contains_key("b"));
    }

    #[test]
    fn subtable_headers_are_scoped_to_their_array_element() {
        let text = "[[a]]\n[a.b]\nx = 1\n[[a]]\n[a.b]\nx = 2\n";
        let doc = parse(text, "plenty.toml").unwrap();
        let Value::Array(items) = get(&doc, "a") else {
            panic!()
        };
        for (item, x) in items.iter().zip([1, 2]) {
            let Value::Table(item) = item else { panic!() };
            let Value::Table(b) = get(item, "b") else {
                panic!()
            };
            assert_eq!(get(b, "x"), &Value::Integer(x));
        }
        // Within one element the header is still a duplicate, and a new
        // element of an unrelated array does not reset it.
        for text in [
            "[[a]]\n[a.b]\n[a.b]\n",
            "[a.b]\n[[c]]\n[a.b]\n",
            "[[a]]\n[a.b]\n[[ab]]\n[a.b]\n",
        ] {
            let error = parse(text, "plenty.toml").unwrap_err();
            assert!(
                error.contains("table `a.b` is defined twice"),
                "{text:?}: {error}"
            );
        }
    }

    #[test]
    fn errors_name_the_line() {
        for (text, message) in [
            ("a = 1\na = 2\n", "plenty.toml:2: key `a` is defined twice"),
            ("[x]\n[x]\n", "plenty.toml:2: table `x` is defined twice"),
            ("a.b = 1\n", "plenty.toml:1: dotted keys are not supported"),
            ("a = 1.5\n", "plenty.toml:1: only decimal integers"),
            ("a = \"x\n", "plenty.toml:1: unterminated string"),
            ("a = 1 b\n", "plenty.toml:1: expected the end of the line"),
            ("a = 1\n[a]\n", "plenty.toml:2: `a` is not a table"),
            (
                "[[a]]\nx = 1\n[a]\ny = 2\n",
                "plenty.toml:3: `a` is already defined as an array",
            ),
            (
                "[x]\n[[x.a]]\n[x.a]\n",
                "plenty.toml:3: `x.a` is already defined as an array",
            ),
            // TOML forbids extending or descending into a static array.
            (
                "a = []\n[[a]]\n",
                "plenty.toml:2: `a` is already defined as a value",
            ),
            (
                "a = [{ x = 1 }]\n[[a]]\n",
                "plenty.toml:2: `a` is already defined as a value",
            ),
            (
                "a = [{ x = 1 }]\n[a.b]\n",
                "plenty.toml:2: `a` is not a table",
            ),
            (
                "a = [{ x = 1 }]\n[[a.b]]\n",
                "plenty.toml:2: `a` is not a table",
            ),
            (
                "a = \"\"\"x\"\"\"\n",
                "multi-line strings are not supported",
            ),
            ("a = 007\n", "only decimal integers"),
            ("a = \"\\u+041\"\n", "plenty.toml:1: invalid \\u escape"),
        ] {
            let error = parse(text, "plenty.toml").unwrap_err();
            assert!(error.contains(message), "{text:?}: {error}");
        }
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let text = format!("a = {}{}\n", "[".repeat(100), "]".repeat(100));
        assert!(parse(&text, "m").unwrap_err().contains("nested too deeply"));
    }

    #[test]
    fn quoting_roundtrips() {
        let text = "tab\t \"quote\" back\\slash \u{1} é";
        let doc = parse(&format!("a = {}\n", quote(text)), "m").unwrap();
        assert_eq!(get(&doc, "a"), &Value::String(text.into()));
    }
}

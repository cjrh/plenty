//! The syntax layer: turns raw source text into a flat stream of words and
//! string literals.

use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// One lexical unit of Plenty source.
///
/// Plenty's grammar is "whitespace-separated words" plus one form of string
/// literal. A token borrows a slice of the source; no token owns or copies
/// text. Escape sequences inside `"..."` are not interpreted
/// here — they are resolved when the compiler interns the text into the heap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok<'a> {
    /// Raw inner content of a `"..."` literal; `\"` and `\\` escapes
    /// not yet decoded.
    Text(&'a str),
    /// An unquoted word — a number, an operator, or a name. Resolved later,
    /// when compiled to an [`Op`](crate::op::Op).
    Word(&'a str),
}

/// Split `source` into tokens.
///
/// Whitespace separates words. `#` starts a comment that runs to the next
/// newline. `{`, `}`, `[`, `]`, and `;` are standalone structural words even
/// without surrounding whitespace. A `"` opens a string literal that runs to
/// the next unescaped `"`, capturing everything between verbatim — newlines,
/// spaces, comment markers, and operator characters all included. Inside the
/// literal, `\X` consumes both characters without interpreting them, so `\"`
/// does not close the string. The only lex error is an unterminated string
/// literal.
pub fn lex(source: &str) -> Result<Vec<Tok<'_>>> {
    fn is_structural(c: char) -> bool {
        matches!(c, '{' | '}' | '[' | ']' | ';')
    }

    let mut toks = Vec::new();
    let mut iter = source.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '#' {
            for (_, c) in iter.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if is_structural(c) {
            toks.push(Tok::Word(&source[i..i + c.len_utf8()]));
            continue;
        }
        if c == '"' {
            let unterminated = || -> Box<dyn Error> {
                let before = &source[..i];
                let line = before.matches('\n').count() + 1;
                let line_start = before.rfind('\n').map_or(0, |n| n + 1);
                let column = before[line_start..].chars().count() + 1;
                format!("{line}:{column}: unterminated string literal").into()
            };
            let start = i + 1;
            let end;
            loop {
                match iter.next() {
                    Some((j, '"')) => {
                        end = j;
                        break;
                    }
                    Some((_, '\\')) => {
                        if iter.next().is_none() {
                            return Err(unterminated());
                        }
                    }
                    Some(_) => continue,
                    None => return Err(unterminated()),
                }
            }
            toks.push(Tok::Text(&source[start..end]));
        } else {
            let start = i;
            let end;
            loop {
                match iter.peek() {
                    Some(&(j, c2))
                        if c2.is_whitespace() || c2 == '"' || c2 == '#' || is_structural(c2) =>
                    {
                        end = j;
                        break;
                    }
                    Some(_) => {
                        iter.next();
                    }
                    None => {
                        end = source.len();
                        break;
                    }
                }
            }
            toks.push(Tok::Word(&source[start..end]));
        }
    }
    Ok(toks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The error names the opening quote's line and character column, in
    /// the modern frontend's `line:column` form (issue #181).
    #[test]
    fn unterminated_string_reports_opening_quote() {
        for (source, location) in [
            ("\"abc", "1:1"),
            ("1 .\n  \"tail\\", "2:3"),
            ("\"é\" é \"x", "1:7"),
        ] {
            let error = lex(source).unwrap_err().to_string();
            assert_eq!(error, format!("{location}: unterminated string literal"));
        }
    }
}

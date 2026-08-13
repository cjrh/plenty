//! The syntax layer: turns raw source text into a flat stream of words and
//! string literals.

use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// One lexical unit of Plenty source.
///
/// Plenty's grammar is "whitespace-separated words" plus one form of string
/// literal. A token carries a borrowed slice of the source; the lexer
/// allocates nothing. Escape sequences inside `"..."` are not interpreted
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
                            return Err("unterminated string literal".into());
                        }
                    }
                    Some(_) => continue,
                    None => return Err("unterminated string literal".into()),
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

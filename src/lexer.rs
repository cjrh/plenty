//! The syntax layer: turns raw source text into a flat stream of words,
//! applying Plenty's quoting rules so no later stage has to think about them.

/// One lexical unit of Plenty source.
///
/// Plenty's grammar is almost entirely "whitespace-separated words"; the only
/// real syntax is quoting. So a token is just a borrowed slice of the source,
/// labelled as either data or code. The lexer allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok<'a> {
    /// A text literal: `` `word `` on its own, or any word between a bare
    /// `` ` `` and a `~`. Always data.
    Text(&'a str),
    /// An unquoted word — a number, an operator, or a name. Its meaning is
    /// resolved later, when the word is compiled to an [`Op`](crate::op::Op).
    Word(&'a str),
}

/// Split `source` into tokens, resolving the quoting rules.
///
/// Never fails: every sequence of characters is lexically valid. An unclosed
/// literal section (a `` ` `` with no matching `~`) simply runs to end of input.
pub fn lex(source: &str) -> Vec<Tok<'_>> {
    let mut toks = Vec::new();
    let mut in_literal = false;
    for word in source.split_whitespace() {
        if in_literal {
            match word {
                "~" => in_literal = false,
                _ => toks.push(Tok::Text(word)),
            }
        } else if word == "`" {
            in_literal = true;
        } else if let Some(rest) = word.strip_prefix('`') {
            toks.push(Tok::Text(rest));
        } else {
            toks.push(Tok::Word(word));
        }
    }
    toks
}

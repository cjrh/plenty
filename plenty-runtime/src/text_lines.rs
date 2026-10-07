//! Borrowed, allocation-free scanning of Python-style text line boundaries.
#[derive(Clone)]
pub(crate) struct Lines<'a> {
    remaining: &'a str,
    keep_ends: bool,
}

impl<'a> Lines<'a> {
    pub(crate) fn new(text: &'a str, keep_ends: bool) -> Self {
        Self {
            remaining: text,
            keep_ends,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        let text = self.remaining;
        for (index, ch) in text.char_indices() {
            if matches!(
                ch,
                '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}'
                    ..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}'
            ) {
                let mut end = index + ch.len_utf8();
                if ch == '\r' && text[end..].starts_with('\n') {
                    end += 1;
                }
                self.remaining = &text[end..];
                return Some(&text[..if self.keep_ends { end } else { index }]);
            }
        }
        self.remaining = "";
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::Lines;

    #[test]
    fn boundaries_preserve_original_terminators_and_do_not_invent_eof_lines() {
        for separator in [
            "\n", "\r", "\r\n", "\u{b}", "\u{c}", "\u{1c}", "\u{1d}", "\u{1e}", "\u{85}",
            "\u{2028}", "\u{2029}",
        ] {
            let input = format!("é{separator}{separator}🦀{separator}");
            assert_eq!(
                Lines::new(&input, false).collect::<Vec<_>>(),
                ["é", "", "🦀"]
            );
            assert_eq!(
                Lines::new(&input, true).collect::<Vec<_>>(),
                [
                    format!("é{separator}"),
                    separator.to_owned(),
                    format!("🦀{separator}")
                ]
            );
        }
        assert_eq!(Lines::new("", false).count(), 0);
        assert_eq!(Lines::new("\n", false).collect::<Vec<_>>(), [""]);
        assert_eq!(Lines::new("a\0b\t", false).collect::<Vec<_>>(), ["a\0b\t"]);
    }
}

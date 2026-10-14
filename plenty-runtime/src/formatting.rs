//! A bounded float spelling primitive; output policy belongs to checked std.fmt.
use std::fmt::{self, Write};

struct Spelling<'a> {
    bytes: &'a mut [u8],
    len: usize,
}

impl Write for Spelling<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let destination = self
            .bytes
            .get_mut(self.len..self.len + text.len())
            .ok_or(fmt::Error)?;
        destination.copy_from_slice(text.as_bytes());
        self.len += text.len();
        Ok(())
    }
}

/// Scientific shortest-round-trip f64 spellings fit within 32 ASCII bytes.
/// The caller supplies initialized writable storage; no capacity growth occurs.
pub(crate) fn write_float(bytes: &mut [u8], value: f64) -> i64 {
    if bytes.len() < 32 {
        return -1;
    }
    let mut spelling = Spelling { bytes, len: 0 };
    if write!(spelling, "{value:e}").is_err() {
        return -1;
    }
    spelling.len as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_spelling_covers_extremes_and_short_buffers() {
        for value in [
            0.0,
            -0.0,
            f64::MIN,
            f64::MAX,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ] {
            let expected = format!("{value:e}");
            let mut buffer = [0; 32];
            let size = write_float(&mut buffer, value);
            assert_eq!(size as usize, expected.len());
            assert_eq!(&buffer[..size as usize], expected.as_bytes());
            let mut short = [42; 31];
            assert_eq!(write_float(&mut short, value), -1);
            assert_eq!(short, [42; 31]);
        }
    }
}

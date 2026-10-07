//! Numeric text conversion uses stack storage and inline error values.
use crate::aggregates::wrap;
use crate::{
    memory::AllocError,
    strings::{self, Text},
};
use std::fmt::Write;

/// Shortest numeric rendering needs fewer than 128 bytes for supported widths.
/// The formatter can fail rather than grow an intermediate heap buffer.
pub(crate) fn format(value: u128, kind: u8) -> Result<*mut Text, AllocError> {
    struct Buffer {
        bytes: [u8; 128],
        len: usize,
    }
    impl Write for Buffer {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            let end = self.len.checked_add(text.len()).ok_or(std::fmt::Error)?;
            self.bytes
                .get_mut(self.len..end)
                .ok_or(std::fmt::Error)?
                .copy_from_slice(text.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut out = Buffer {
        bytes: [0; 128],
        len: 0,
    };
    let result = match kind {
        b'1' => write!(out, "{}", value as i8),
        b'2' => write!(out, "{}", value as i16),
        b'3' => write!(out, "{}", value as i32),
        b'4' => write!(out, "{}", value as i64),
        b'5' => write!(out, "{}", value as u8),
        b'6' => write!(out, "{}", value as u16),
        b'7' => write!(out, "{}", value as u32),
        b'8' => write!(out, "{}", value as u64),
        b'f' => write!(out, "{:?}", f32::from_bits(value as u32)),
        b'd' => write!(out, "{:?}", f64::from_bits(value as u64)),
        b'b' => out.write_str(if value == 0 { "False" } else { "True" }),
        _ => unreachable!("scalar descriptor"),
    };
    result.map_err(|_| AllocError::CapacityOverflow)?;
    strings::try_new(std::str::from_utf8(&out.bytes[..out.len]).expect("formatter emits UTF-8"))
}

pub(crate) fn parse(text: &str, kind: u8) -> u128 {
    let text = text.trim();
    if matches!(kind, b'f' | b'd') {
        let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
        let explicit_infinity =
            unsigned.eq_ignore_ascii_case("inf") || unsigned.eq_ignore_ascii_case("infinity");
        let value = if kind == b'f' {
            text.parse::<f32>()
                .map(|n| (n.to_bits() as u128, n.is_infinite()))
        } else {
            text.parse::<f64>()
                .map(|n| (n.to_bits() as u128, n.is_infinite()))
        };
        return match value {
            Ok((_, true)) if !explicit_infinity => wrap(wrap(0, 1), 1),
            Ok((value, _)) => wrap(value, 0),
            Err(_) => wrap(wrap(0, 0), 1),
        };
    }
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return wrap(wrap(0, 0), 1);
    }
    macro_rules! integer {
        ($t:ty) => {
            text.parse::<$t>().map(|n| n as u64 as u128)
        };
    }
    let value = match kind {
        b'1' => integer!(i8),
        b'2' => integer!(i16),
        b'3' => integer!(i32),
        b'4' => integer!(i64),
        b'5' => integer!(u8),
        b'6' => integer!(u16),
        b'7' => integer!(u32),
        b'8' => integer!(u64),
        _ => unreachable!("numeric descriptor"),
    };
    match value {
        Ok(value) => wrap(value, 0),
        Err(_) => wrap(wrap(0, 1), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_and_invalid_input() {
        assert_eq!(parse(" -128 ", b'1'), (-128i64) as u64 as u128);
        assert_eq!(parse("18446744073709551615", b'8'), u64::MAX as u128);
        assert_eq!(parse("128", b'1'), 3u128 << 64);
        assert_eq!(parse("-1", b'8'), 3u128 << 64);
        for text in ["", "+", "1_0", "1.0", "１２", "2\0", "1 2"] {
            assert_eq!(parse(text, b'4'), 1u128 << 64, "{text}");
        }
    }

    #[test]
    fn float_rounding_special_values_and_range() {
        assert_eq!(parse("-0", b'f'), (-0f32).to_bits() as u128);
        assert_eq!(parse(".125", b'd'), 0.125f64.to_bits() as u128);
        assert_eq!(parse("1e100", b'f'), 3u128 << 64);
        assert_eq!(parse("1e-1000", b'd'), 0);
        assert_eq!(parse("+INFINITY", b'f'), f32::INFINITY.to_bits() as u128);
        assert!(f64::from_bits(parse("NaN", b'd') as u64).is_nan());
        for text in ["", "1_0", "0x10", "1 2", "1e", "1\0"] {
            assert_eq!(parse(text, b'd'), 1u128 << 64);
        }
    }
}

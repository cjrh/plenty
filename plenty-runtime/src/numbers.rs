//! Numeric text conversion uses stack storage and inline error values.
use crate::aggregates::wrap;

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

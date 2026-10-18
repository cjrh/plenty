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

/// Spelling styles for [`write_float_spec`]; `ALTERNATE` may be added to any.
pub(crate) const FIXED: i64 = 1;
pub(crate) const EXPONENT: i64 = 2;
pub(crate) const GENERAL: i64 = 3;
/// Python's spec without a type but with a precision: general, keeping `.0`.
pub(crate) const GENERAL_POINT: i64 = 4;
/// The `print` spelling of an f64, or of an f32 widened to f64.
pub(crate) const DISPLAY: i64 = 5;
pub(crate) const DISPLAY_F32: i64 = 6;
pub(crate) const ALTERNATE: i64 = 16;

/// Python format-spec float spellings (std.fmt applies sign options, grouping
/// and padding). A negative value, including -0.0 but not NaN, starts with `-`.
/// Writes at `offset` and returns the length written, or the negated total
/// length (offset included) that `bytes` needs, leaving it unchanged.
/// Precision is nonnegative and caller-bounded; std.bytes checks the offset.
pub(crate) fn write_float_spec(
    bytes: &mut [u8],
    value: f64,
    style: i64,
    precision: i64,
    offset: i64,
) -> i64 {
    let text = float_spec(value, style, precision.max(0) as usize);
    let offset = offset.max(0) as usize;
    match bytes.get_mut(offset..offset + text.len()) {
        Some(destination) => {
            destination.copy_from_slice(text.as_bytes());
            text.len() as i64
        }
        None => -((offset + text.len()) as i64),
    }
}

fn float_spec(value: f64, style: i64, precision: usize) -> String {
    let alternate = style & ALTERNATE != 0;
    let style = style & !ALTERNATE;
    if style == DISPLAY {
        return format!("{value:?}");
    }
    if style == DISPLAY_F32 {
        return format!("{:?}", value as f32);
    }
    let mut text = String::new();
    if value.is_sign_negative() && !value.is_nan() {
        text.push('-');
    }
    let magnitude = value.abs();
    if !magnitude.is_finite() {
        text.push_str(if magnitude.is_nan() { "nan" } else { "inf" });
        return text;
    }
    match style {
        FIXED => {
            text.push_str(&format!("{magnitude:.precision$}"));
            if alternate && precision == 0 {
                text.push('.');
            }
        }
        EXPONENT => {
            let (mantissa, exponent) = exponent_parts(magnitude, precision);
            text.push_str(&mantissa);
            if alternate && precision == 0 {
                text.push('.');
            }
            push_exponent(&mut text, exponent);
        }
        _ => {
            debug_assert!(matches!(style, GENERAL | GENERAL_POINT));
            // Python's `g`: round to `precision` significant digits, then pick
            // fixed or exponent notation from the rounded exponent.
            let point = style == GENERAL_POINT;
            let significant = precision.max(1);
            let (mantissa, exponent) = exponent_parts(magnitude, significant - 1);
            let limit = if point {
                significant as i64 - 1
            } else {
                significant as i64
            };
            if exponent < -4 || exponent >= limit {
                text.push_str(&trimmed(mantissa, alternate));
                push_exponent(&mut text, exponent);
            } else {
                let decimals = (significant as i64 - 1 - exponent) as usize;
                let mut fixed = trimmed(format!("{magnitude:.decimals$}"), alternate);
                if point && !fixed.contains('.') {
                    fixed.push_str(".0");
                }
                text.push_str(&fixed);
            }
        }
    }
    text
}

/// The `d.ddd` mantissa and decimal exponent after rounding to `precision` decimals.
fn exponent_parts(magnitude: f64, precision: usize) -> (String, i64) {
    let spelled = format!("{magnitude:.precision$e}");
    let (mantissa, exponent) = spelled.split_once('e').expect("exponent spelling");
    (mantissa.into(), exponent.parse().expect("decimal exponent"))
}

/// Remove trailing fractional zeros and a bare point, unless `#` keeps them.
fn trimmed(mut text: String, alternate: bool) -> String {
    if alternate {
        if !text.contains('.') {
            text.push('.');
        }
        return text;
    }
    if text.contains('.') {
        let kept = text.trim_end_matches('0').trim_end_matches('.').len();
        text.truncate(kept);
    }
    text
}

/// Python spells exponents with a sign and at least two digits.
fn push_exponent(text: &mut String, exponent: i64) {
    let sign = if exponent < 0 { '-' } else { '+' };
    text.push_str(&format!("e{sign}{:02}", exponent.abs()));
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

    fn spec(value: f64, style: i64, precision: i64) -> String {
        let mut buffer = [0; 400];
        let size = write_float_spec(&mut buffer, value, style, precision, 0);
        String::from_utf8(buffer[..size as usize].to_vec()).unwrap()
    }

    #[test]
    fn python_spellings() {
        assert_eq!(spec(1.23456, FIXED, 2), "1.23");
        assert_eq!(spec(2.675, FIXED, 2), "2.67");
        assert_eq!(spec(-0.0, FIXED, 1), "-0.0");
        assert_eq!(spec(3.0, FIXED | ALTERNATE, 0), "3.");
        assert_eq!(spec(1234.5, EXPONENT, 2), "1.23e+03");
        assert_eq!(spec(1e-10, EXPONENT, 0), "1e-10");
        assert_eq!(spec(1e100, EXPONENT, 1), "1.0e+100");
        assert_eq!(spec(1234.5, GENERAL, 6), "1234.5");
        assert_eq!(spec(0.00001, GENERAL, 6), "1e-05");
        assert_eq!(spec(123456789.0, GENERAL, 6), "1.23457e+08");
        assert_eq!(spec(0.0001, GENERAL, 3), "0.0001");
        assert_eq!(spec(1.0, GENERAL | ALTERNATE, 3), "1.00");
        assert_eq!(spec(9.9996, GENERAL, 4), "10");
        assert_eq!(spec(123.0, GENERAL_POINT, 3), "1.23e+02");
        assert_eq!(spec(12.0, GENERAL_POINT, 3), "12.0");
        assert_eq!(spec(f64::NAN, FIXED, 2), "nan");
        assert_eq!(spec(f64::NEG_INFINITY, GENERAL, 2), "-inf");
        assert_eq!(spec(1e16, DISPLAY, 0), "1e16");
        assert_eq!(spec(0.1f32 as f64, DISPLAY_F32, 0), "0.1");
        let mut short = [7; 3];
        assert_eq!(write_float_spec(&mut short, 1234.5, FIXED, 1, 2), -8);
        assert_eq!(short, [7; 3]);
    }
}

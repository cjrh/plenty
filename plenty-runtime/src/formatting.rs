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

/// Largest precision std.bytes passes to [`write_float_spec`].
const MAX_PRECISION: usize = 4096;
/// The longest spelling is fixed notation of `-f64::MAX` (310 bytes before the
/// point) with `MAX_PRECISION` decimals; the slack also covers `#` and rounding.
const SPELLING_CAPACITY: usize = MAX_PRECISION + 320;

/// Python format-spec float spellings (std.fmt applies sign options, grouping
/// and padding). A negative value, including -0.0 but not NaN, starts with `-`.
/// Writes at `offset` and returns the length written, or the negated total
/// length (offset included) that `bytes` needs, leaving it unchanged.
/// Precision is nonnegative and caller-bounded; std.bytes checks the offset.
/// The spelling is built on the stack: a heap allocation here could not report
/// refusal as `BytesError.Allocation` (issue #298).
pub(crate) fn write_float_spec(
    bytes: &mut [u8],
    value: f64,
    style: i64,
    precision: i64,
    offset: i64,
) -> i64 {
    let mut storage = [0; SPELLING_CAPACITY];
    let mut spelling = Spelling {
        bytes: &mut storage,
        len: 0,
    };
    let precision = (precision.max(0) as usize).min(MAX_PRECISION);
    if float_spec(&mut spelling, value, style, precision).is_err() {
        crate::fail("float spelling exceeds its bound");
    }
    let text = &spelling.bytes[..spelling.len];
    let offset = offset.max(0) as usize;
    match bytes.get_mut(offset..offset + text.len()) {
        Some(destination) => {
            destination.copy_from_slice(text);
            text.len() as i64
        }
        None => -((offset + text.len()) as i64),
    }
}

fn float_spec(text: &mut Spelling, value: f64, style: i64, precision: usize) -> fmt::Result {
    let alternate = style & ALTERNATE != 0;
    let style = style & !ALTERNATE;
    if style == DISPLAY {
        return write!(text, "{value:?}");
    }
    if style == DISPLAY_F32 {
        return write!(text, "{:?}", value as f32);
    }
    if value.is_sign_negative() && !value.is_nan() {
        text.write_char('-')?;
    }
    let magnitude = value.abs();
    if !magnitude.is_finite() {
        return text.write_str(if magnitude.is_nan() { "nan" } else { "inf" });
    }
    match style {
        FIXED => {
            write!(text, "{magnitude:.precision$}")?;
            if alternate && precision == 0 {
                text.write_char('.')?;
            }
        }
        EXPONENT => {
            let exponent = exponent_parts(text, magnitude, precision)?;
            if alternate && precision == 0 {
                text.write_char('.')?;
            }
            push_exponent(text, exponent)?;
        }
        _ => {
            debug_assert!(matches!(style, GENERAL | GENERAL_POINT));
            // Python's `g`: round to `precision` significant digits, then pick
            // fixed or exponent notation from the rounded exponent.
            let point = style == GENERAL_POINT;
            let significant = precision.max(1);
            let start = text.len;
            let exponent = exponent_parts(text, magnitude, significant - 1)?;
            let limit = if point {
                significant as i64 - 1
            } else {
                significant as i64
            };
            if exponent < -4 || exponent >= limit {
                trim(text, start, alternate)?;
                push_exponent(text, exponent)?;
            } else {
                text.len = start;
                let decimals = (significant as i64 - 1 - exponent) as usize;
                write!(text, "{magnitude:.decimals$}")?;
                trim(text, start, alternate)?;
                if point && !text.bytes[start..text.len].contains(&b'.') {
                    text.write_str(".0")?;
                }
            }
        }
    }
    Ok(())
}

/// Writes the `d.ddd` mantissa after rounding to `precision` decimals and
/// returns its decimal exponent.
fn exponent_parts(
    text: &mut Spelling,
    magnitude: f64,
    precision: usize,
) -> Result<i64, fmt::Error> {
    let start = text.len;
    write!(text, "{magnitude:.precision$e}")?;
    let marker = start
        + text.bytes[start..text.len]
            .iter()
            .position(|&byte| byte == b'e')
            .expect("exponent spelling");
    let exponent = std::str::from_utf8(&text.bytes[marker + 1..text.len])
        .ok()
        .and_then(|digits| digits.parse().ok())
        .expect("decimal exponent");
    text.len = marker;
    Ok(exponent)
}

/// Remove trailing fractional zeros and a bare point from the spelling after
/// `start`, unless `#` keeps them.
fn trim(text: &mut Spelling, start: usize, alternate: bool) -> fmt::Result {
    let has_point = text.bytes[start..text.len].contains(&b'.');
    if alternate {
        if !has_point {
            text.write_char('.')?;
        }
        return Ok(());
    }
    if has_point {
        while text.bytes[text.len - 1] == b'0' {
            text.len -= 1;
        }
        if text.bytes[text.len - 1] == b'.' {
            text.len -= 1;
        }
    }
    Ok(())
}

/// Python spells exponents with a sign and at least two digits.
fn push_exponent(text: &mut Spelling, exponent: i64) -> fmt::Result {
    let sign = if exponent < 0 { '-' } else { '+' };
    write!(text, "e{sign}{:02}", exponent.abs())
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

    #[test]
    fn longest_spellings_fit_the_stack_buffer() {
        let mut buffer = vec![0; SPELLING_CAPACITY];
        for (style, length) in [
            (FIXED | ALTERNATE, 310 + 1 + MAX_PRECISION),
            (EXPONENT | ALTERNATE, 3 + MAX_PRECISION + 5),
            (GENERAL | ALTERNATE, 1 + MAX_PRECISION + 1),
        ] {
            let size = write_float_spec(&mut buffer, -f64::MAX, style, MAX_PRECISION as i64, 0);
            assert_eq!(size as usize, length, "style {style}");
        }
        let size = write_float_spec(&mut buffer, 1e-5, GENERAL_POINT, MAX_PRECISION as i64, 0);
        assert!(buffer[..size as usize].starts_with(b"1.0000000000000000818"));
    }
}

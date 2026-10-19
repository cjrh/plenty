//! Primitives for `std.regex`: Unicode properties from the toolchain's `core`
//! tables, simple case mappings, and reads of borrowed UTF-8 text by byte
//! offset. They do not allocate, except `TextSlice` (op 363), which copies a
//! byte range into a new string and can fail with an allocation error.
//! Parsing, compilation and matching are Plenty.
use crate::strings::{self, Text};

fn property(code: u32, property: i64) -> bool {
    let Some(scalar) = char::from_u32(code) else {
        return false;
    };
    match property {
        10 => scalar.is_alphabetic(),
        11 => scalar.is_lowercase(),
        12 => scalar.is_uppercase(),
        13 => scalar.is_whitespace(),
        14 => scalar.is_numeric(),
        15 => scalar.is_control(),
        _ => crate::fail("invalid regex property"),
    }
}

/// The mapping when it is exactly one scalar, otherwise the scalar itself.
fn case(code: u32, upper: bool) -> u32 {
    let Some(scalar) = char::from_u32(code) else {
        return code;
    };
    let mut mapped = [None, None];
    let mut count = 0;
    let mut record = |c: char| {
        if count < 2 {
            mapped[count] = Some(c);
        }
        count += 1;
    };
    if upper {
        scalar.to_uppercase().for_each(&mut record);
    } else {
        scalar.to_lowercase().for_each(&mut record);
    }
    match (count, mapped[0]) {
        (1, Some(c)) => c as u32,
        _ => code,
    }
}

fn at(text: &str, offset: i64) -> i64 {
    if offset == text.len() as i64 {
        return -1;
    }
    let Some(tail) = usize::try_from(offset)
        .ok()
        .filter(|&offset| text.is_char_boundary(offset))
        .and_then(|offset| text.get(offset..))
    else {
        return -2;
    };
    let scalar = tail.chars().next().unwrap();
    (scalar as i64) * 8 + scalar.len_utf8() as i64
}

pub(crate) unsafe fn call(op: i64, a: u128, b: u128, c: u128) -> u128 {
    if op == 360 {
        return property(a as u32, b as i64) as u128;
    }
    if op == 365 {
        return case(a as u32, b != 0) as u128;
    }
    let text = a as *const Text;
    // SAFETY: the compiler passes a borrowed text word that outlives the call.
    let source = unsafe { strings::bytes(&text) };
    // SAFETY: `str` values are always valid UTF-8.
    let source = unsafe { std::str::from_utf8_unchecked(source) };
    match op {
        361 => at(source, b as i64) as u128,
        362 => usize::try_from(b as i64)
            .ok()
            .and_then(|offset| source.as_bytes().get(offset))
            .map_or(-1, |&byte| byte as i64) as u128,
        363 => {
            let range = usize::try_from(b as i64)
                .ok()
                .zip(usize::try_from(c as i64).ok())
                .and_then(|(start, stop)| source.get(start..stop))
                .unwrap_or_else(|| crate::fail("invalid regex text range"));
            match strings::try_new(range) {
                Ok(text) => crate::aggregates::wrap(text as u128, 0),
                Err(error) => crate::aggregates::wrap(crate::aggregates::wrap(0, error as u64), 1),
            }
        }
        364 => {
            let needle = b as *const Text;
            // SAFETY: as for the haystack.
            let needle = unsafe { std::str::from_utf8_unchecked(strings::bytes(&needle)) };
            usize::try_from(c as i64)
                .ok()
                .and_then(|start| Some(start + source.get(start..)?.find(needle)?))
                .map_or(-1, |offset| offset as i64) as u128
        }
        _ => crate::fail("invalid regex operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_cases_and_offsets() {
        // Properties and case mappings follow Rust's Unicode tables, independently
        // of std.unicode's vendored normalization and Mark tables.
        assert!(property('é' as u32, 10) && !property('1' as u32, 10));
        assert!(property('٣' as u32, 14) && property('Ⅻ' as u32, 14));
        assert!(property(0xA0, 13) && !property(0x200B, 13));
        assert!(property('Σ' as u32, 12) && property('σ' as u32, 11));
        assert!(!property(0x11_0000, 10));
        assert_eq!(case('Σ' as u32, false), 'σ' as u32);
        assert_eq!(case(0x212A, false), 'k' as u32);
        assert_eq!(case('ß' as u32, true), 'ß' as u32);
        assert_eq!(at("aé", 0), 'a' as i64 * 8 + 1);
        assert_eq!(at("aé", 1), 'é' as i64 * 8 + 2);
        assert_eq!(at("aé", 2), -2);
        assert_eq!(at("aé", 3), -1);
        assert_eq!(at("aé", 4), -2);
        assert_eq!(at("aé", -1), -2);
    }
}

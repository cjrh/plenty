//! Fixed-version scalar primitives. Normalization policy lives in std.unicode.
#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
pub(crate) mod vendor {
    pub(crate) mod lookups;
    pub(crate) mod normalize;
    pub(crate) mod perfect_hash;
    pub(crate) mod tables;
    // The unmodified upstream tables also contain unused quick-check data.
    pub(crate) mod quick_check {
        pub enum IsNormalized { Yes, No, Maybe }
    }
}

const NONE: u32 = 0x11_0000;

// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(crate) fn call(op: i64, a: u128, b: u128, c: u128) -> u128 {
    let scalar = char::from_u32(a as u32).unwrap_or_else(|| crate::fail("invalid Unicode scalar"));
    match op {
        224 => vendor::lookups::canonical_combining_class(scalar) as u128,
        225 => {
            let mut result = NONE;
            let mut index = 0i64;
            let mut emit = |character: char| {
                if index == c as i64 {
                    result = character as u32;
                }
                index += 1;
            };
            if b == 0 {
                vendor::normalize::decompose_canonical(scalar, &mut emit);
            } else {
                vendor::normalize::decompose_compatible(scalar, &mut emit);
            }
            result as u128
        }
        226 => {
            let second =
                char::from_u32(b as u32).unwrap_or_else(|| crate::fail("invalid Unicode scalar"));
            vendor::normalize::compose(scalar, second).map_or(NONE, |ch| ch as u32) as u128
        }
        227 => vendor::lookups::is_combining_mark(scalar) as u128,
        _ => crate::fail("invalid Unicode operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_contract_and_pinned_version() {
        assert_eq!(vendor::tables::UNICODE_VERSION, (17, 0, 0));
        assert_eq!(call(224, 0x301, 0, 0), 230);
        assert_eq!(call(225, 'é' as u128, 0, 0), 'e' as u128);
        assert_eq!(call(225, 'é' as u128, 0, 1), 0x301);
        assert_eq!(call(225, 'é' as u128, 0, 2), NONE as u128);
        assert_eq!(call(225, 'é' as u128, 0, u128::MAX), NONE as u128);
        assert_eq!(call(226, 'e' as u128, 0x301, 0), 'é' as u128);
        assert_eq!(call(226, 'a' as u128, 'b' as u128, 0), NONE as u128);
        // U+0903 is a Mark even though its combining class is zero.
        assert_eq!(call(227, 0x903, 0, 0), 1);
        assert_eq!(call(224, 0x903, 0, 0), 0);
    }
}

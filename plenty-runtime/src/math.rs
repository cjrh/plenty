//! Numeric functions that native code calls instead of expanding inline.
//! Float functions follow IEEE 754 and the platform math library and never
//! trap; integer powers trap where arithmetic would.

/// `kind` is `plenty::numeric::FloatOp`. Unary functions ignore `y`.
macro_rules! float_math {
    ($name:ident, $float:ty) => {
        #[unsafe(no_mangle)]
        pub(crate) extern "C" fn $name(kind: u64, x: $float, y: $float) -> $float {
            match kind {
                0 => x.exp(),
                1 => x.ln(),
                2 => x.sin(),
                3 => x.cos(),
                4 => x.floor(),
                5 => x.ceil(),
                6 => x.powf(y),
                7 => x % y,
                8 => x.tan(),
                9 => x.asin(),
                10 => x.acos(),
                11 => x.atan(),
                12 => x.atan2(y),
                13 => x.sinh(),
                14 => x.cosh(),
                15 => x.tanh(),
                16 => x.asinh(),
                17 => x.acosh(),
                18 => x.atanh(),
                19 => x.exp2(),
                20 => x.exp_m1(),
                21 => x.ln_1p(),
                22 => x.log2(),
                23 => x.log10(),
                24 => x.cbrt(),
                25 => x.trunc(),
                26 => x.abs(),
                27 => x.copysign(y),
                28 => x.round_ties_even(),
                _ => unreachable!("invalid float operation"),
            }
        }
    };
}
float_math!(plenty_f64_math, f64);
float_math!(plenty_f32_math, f32);

/// `base` to the power `exponent` when the result lies in `min..=max`.
/// A square that leaves the range is only an overflow while a later
/// multiplication still needs it.
fn checked_pow(base: i128, mut exponent: u64, min: i128, max: i128) -> Option<i128> {
    let fits = |value: i128| (min..=max).contains(&value).then_some(value);
    let (mut result, mut power) = (1i128, base);
    loop {
        if exponent & 1 == 1 {
            result = fits(result.checked_mul(power)?)?;
        }
        exponent >>= 1;
        if exponent == 0 {
            return Some(result);
        }
        power = power.checked_mul(power).filter(|square| *square <= max)?;
    }
}

/// `kind` is the integer type's width in bits, plus one when it is signed.
/// Both operands arrive extended to 64 bits from that type.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn plenty_int_pow(
    base: u64,
    exponent: u64,
    kind: u64,
    site: *const crate::Site,
) -> u64 {
    let bits = (kind & !1) as u32;
    let (base, min, max) = if kind & 1 == 1 {
        if (exponent as i64) < 0 {
            crate::fail_kind_at(
                crate::actor_heap::TrapKind::Arithmetic,
                site,
                format_args!("negative integer exponent"),
            )
        }
        let max = (1i128 << (bits - 1)) - 1;
        (base as i64 as i128, -max - 1, max)
    } else {
        (base as i128, 0, (1i128 << bits) - 1)
    };
    match checked_pow(base, exponent, min, max) {
        Some(value) => value as u64,
        None => crate::fail_kind_at(
            crate::actor_heap::TrapKind::Arithmetic,
            site,
            format_args!("integer overflow"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::checked_pow;

    #[test]
    fn powers_stay_exact_at_the_edges_of_each_range() {
        let (i8_min, i8_max) = (i8::MIN as i128, i8::MAX as i128);
        assert_eq!(checked_pow(-2, 7, i8_min, i8_max), Some(-128));
        assert_eq!(checked_pow(2, 7, i8_min, i8_max), None);
        assert_eq!(checked_pow(-2, 8, i8_min, i8_max), None);
        assert_eq!(checked_pow(0, 0, i8_min, i8_max), Some(1));
        assert_eq!(checked_pow(0, u64::MAX, i8_min, i8_max), Some(0));
        assert_eq!(checked_pow(-1, u64::MAX, i8_min, i8_max), Some(-1));
        assert_eq!(checked_pow(1, u64::MAX, 0, u8::MAX as i128), Some(1));
        assert_eq!(checked_pow(2, 63, 0, u64::MAX as i128), Some(1 << 63));
        assert_eq!(checked_pow(2, 64, 0, u64::MAX as i128), None);
        assert_eq!(
            checked_pow(u64::MAX as i128, 1, 0, u64::MAX as i128),
            Some(u64::MAX as i128)
        );
        assert_eq!(checked_pow(u64::MAX as i128, 2, 0, u64::MAX as i128), None);
        assert_eq!(
            checked_pow(i64::MIN as i128, 1, i64::MIN as i128, i64::MAX as i128),
            Some(i64::MIN as i128)
        );
        assert_eq!(
            checked_pow(3, 39, i64::MIN as i128, i64::MAX as i128),
            Some(4_052_555_153_018_976_267)
        );
        assert_eq!(checked_pow(3, 40, i64::MIN as i128, i64::MAX as i128), None);
    }
}

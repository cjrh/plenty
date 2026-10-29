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

/// The two words `plenty_int_pow_checked` returns in `rax:rdx`.
#[repr(C)]
pub(crate) struct Power {
    /// The power modulo 2**N, extended to 64 bits from the integer type.
    wrapped: u64,
    /// 1 when the exact power lies outside the integer type.
    overflow: u64,
}

/// `plenty_int_pow` in a `checked:` block: reports overflow to the caller,
/// which exits the block with an `OverflowError`. A negative exponent
/// still traps, since `pow` has no integer result for it.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn plenty_int_pow_checked(
    base: u64,
    exponent: u64,
    kind: u64,
    site: *const crate::Site,
) -> Power {
    let bits = (kind & !1) as u32;
    let signed = kind & 1 == 1;
    let (exact, min, max) = if signed {
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
    let overflow = checked_pow(exact, exponent, min, max).is_none();
    // Arithmetic modulo 2**64 agrees with arithmetic modulo 2**bits in the
    // low bits; extend the result again from the integer type.
    let wrapped = wrapping_pow(base, exponent);
    let shift = 64 - bits;
    let wrapped = if signed {
        (((wrapped << shift) as i64) >> shift) as u64
    } else if bits == 64 {
        wrapped
    } else {
        wrapped & ((1 << bits) - 1)
    };
    Power {
        wrapped,
        overflow: u64::from(overflow),
    }
}

/// `base ** exponent` modulo 2**64.
fn wrapping_pow(mut base: u64, mut exponent: u64) -> u64 {
    let mut result = 1u64;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result.wrapping_mul(base);
        }
        base = base.wrapping_mul(base);
        exponent >>= 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{checked_pow, wrapping_pow};

    #[test]
    fn wrapped_powers_agree_with_repeated_multiplication() {
        assert_eq!(wrapping_pow(3, 5), 243);
        assert_eq!(wrapping_pow(2, 64), 0);
        assert_eq!(
            wrapping_pow(3, 1 << 40),
            3u64.wrapping_pow(1 << 20).wrapping_pow(1 << 20)
        );
        let powered = super::plenty_int_pow_checked(2, 7, 9, std::ptr::null());
        assert_eq!((powered.wrapped as i64, powered.overflow), (-128, 1));
        let powered = super::plenty_int_pow_checked(-2i64 as u64, 7, 9, std::ptr::null());
        assert_eq!((powered.wrapped as i64, powered.overflow), (-128, 0));
        let powered = super::plenty_int_pow_checked(16, 2, 8, std::ptr::null());
        assert_eq!((powered.wrapped, powered.overflow), (0, 1));
    }

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

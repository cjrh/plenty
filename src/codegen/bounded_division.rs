//! Unsigned division and remainder by a constant, with the multiplier chosen
//! from the dividend's known bound (issues #953 and #954).
//!
//! Cranelift replaces `udiv x, d` by a multiply-high from `d` alone. For
//! divisors such as 1000 the multiplier that is right for every 64-bit `x`
//! needs 65 bits, which costs `mulq; mov; sub; shr; add; shr`. When `x` is
//! known to be at most `bound`, a one-word multiplier often exists, and
//! always when `bound` is below `2**(N - 1)`: `mulq; shr`, or `mulq` alone.
//! gcc (`choose_multiplier` with the dividend's precision) and LLVM
//! (`UnsignedDivisionByConstantInfo::get` with its known leading zeros) use
//! the same information.
//!
//! # Proof
//!
//! Let `N` be the width, `k = N + s`, `m = ceil(2**k / d)` and
//! `e = m * d - 2**k`, so `0 <= e < d`. For `x = q * d + r` with
//! `0 <= r < d`:
//!
//! ```text
//! x * m / 2**k = x / d + x * e / (d * 2**k) = q + (r + x * e / 2**k) / d
//! ```
//!
//! The left side is at least `q`. It is below `q + 1` when
//! `r + x * e / 2**k < d`, and `r <= d - 1`, so `x * e < 2**k` suffices.
//! [`multiplier`] returns the smallest `s` for which `m` fits in `N` bits and
//! `bound * e < 2**k`; then `floor(x * m / 2**k) = x // d` for every
//! `0 <= x <= bound`. The high word of the `N`-bit product is
//! `floor(x * m / 2**N)`, and shifting it right by `s` gives the quotient.
//!
//! With `s = floor(log2(d))` and `d` not a power of two, `m` is below `2**N`
//! and `e < d < 2**(s + 1)`, so any `bound < 2**(N - 1)` satisfies the
//! condition: a nonnegative signed dividend always has such a multiplier.
//!
//! # Remainder
//!
//! The remainder is `x - q * d`, or the equal (modulo `2**N`)
//! `q * (-d) + x`. The subtract must overwrite the dividend's register, so
//! regalloc2 adds two register moves; the add may write either operand's
//! (issue #954). The moves are eliminated at register renaming and cost no
//! latency. The add is used unless `x` is itself a sum with a constant:
//! Cranelift then folds that constant into the add as a three-component
//! `lea`, which is a cycle slower than `sub` on the dependency chain of a
//! generator such as `seed = (seed * a + c) % m`. See
//! `experiments/kernels/field-rng/README.md` for the measurements.
use cranelift_codegen::ir::{InstBuilder, InstructionData, Opcode, Value, ValueDef};

use super::{Lowerer, known};

/// The multiplier `m` and shift `s` with `((x * m) >> bits) >> s == x / d`
/// for every `0 <= x <= bound`, where the product has `2 * bits` bits.
/// `None` when the divisor is a power of two or below 3, which shifts and
/// masks serve, or when no `bits`-wide multiplier is proved for the bound.
pub(super) fn multiplier(divisor: u64, bound: u64, bits: u32) -> Option<(u64, u32)> {
    debug_assert!(matches!(bits, 8 | 16 | 32 | 64));
    let limit = 1u128 << bits;
    let (d, bound) = (u128::from(divisor), u128::from(bound));
    if divisor < 3 || divisor.is_power_of_two() || d >= limit || bound >= limit {
        return None;
    }
    for shift in 0..=divisor.ilog2() {
        // `shift <= 63` and `bits <= 64`, so every quantity fits in 128
        // bits: `power < 2**128`, and `m * d < power + d`.
        let power = 1u128 << (bits + shift);
        let m = power.div_ceil(d);
        if m >= limit {
            return None;
        }
        let excess = m * d - power;
        if bound * excess < power {
            return Some((m as u64, shift));
        }
    }
    None
}

impl Lowerer<'_, '_> {
    /// The quotient of unsigned `a // b` by multiplication, when `b` is a
    /// constant and a one-word multiplier is proved for every value `a` can
    /// hold.
    ///
    /// Returns the quotient and `-b` in the operands' width.
    fn multiplied_quotient(&mut self, a: Value, b: Value) -> Option<(Value, i64)> {
        if !self.module.optimizes() {
            return None;
        }
        let dfg = &self.bcx.func.dfg;
        let ty = dfg.value_type(a);
        // Cranelift expands 32- and 64-bit divisions by constants; narrower
        // ones keep the division instruction.
        if !matches!(ty.bits(), 32 | 64) {
            return None;
        }
        let divisor = known::known_int(dfg, b)?;
        let type_max = u64::MAX >> (64 - ty.bits());
        let bound = self.upper_bound(a).map_or(type_max, |b| b.min(type_max));
        let (m, shift) = multiplier(divisor, bound, ty.bits())?;
        // `iconst` takes the constant's bits zero-extended.
        let m = self.bcx.ins().iconst(ty, m as i64);
        let high = self.bcx.ins().umulhi(a, m);
        let quotient = match shift {
            0 => high,
            _ => self.bcx.ins().ushr_imm_u(high, i64::from(shift)),
        };
        Some((quotient, (divisor.wrapping_neg() & type_max) as i64))
    }

    /// Unsigned `a // b`: both operands are of an unsigned type, or proved
    /// nonnegative.
    pub(super) fn unsigned_quotient(&mut self, a: Value, b: Value) -> Value {
        match self.multiplied_quotient(a, b) {
            Some((quotient, _)) => quotient,
            None => self.bcx.ins().udiv(a, b),
        }
    }

    /// Unsigned `a % b`, under the conditions of [`Self::unsigned_quotient`].
    pub(super) fn unsigned_remainder(&mut self, a: Value, b: Value) -> Value {
        let Some((quotient, negated)) = self.multiplied_quotient(a, b) else {
            return self.bcx.ins().urem(a, b);
        };
        if self.adds_constant(a) {
            let product = self.bcx.ins().imul(quotient, b);
            return self.bcx.ins().isub(a, product);
        }
        let ty = self.bcx.func.dfg.value_type(a);
        let negated = self.bcx.ins().iconst(ty, negated);
        let product = self.bcx.ins().imul(quotient, negated);
        self.bcx.ins().iadd(product, a)
    }

    /// Whether `value` is a sum or difference with a constant operand, which
    /// the x86-64 backend would fold into an address computation that uses it.
    fn adds_constant(&self, value: Value) -> bool {
        let dfg = &self.bcx.func.dfg;
        let ValueDef::Result(inst, _) = dfg.value_def(value) else {
            return false;
        };
        match dfg.insts[inst] {
            InstructionData::Binary {
                opcode: Opcode::Iadd | Opcode::Isub,
                args,
            } => args.iter().any(|arg| known::known_int(dfg, *arg).is_some()),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::multiplier;

    /// `((x * m) >> bits) >> s`, as the emitted instructions compute it.
    fn quotient(x: u64, m: u64, shift: u32, bits: u32) -> u64 {
        ((u128::from(x) * u128::from(m)) >> bits >> shift) as u64
    }

    #[test]
    fn every_eight_bit_multiplier_divides_every_dividend_within_its_bound() {
        for d in 0..=255u64 {
            for bound in 0..=255u64 {
                let Some((m, shift)) = multiplier(d, bound, 8) else {
                    // A nonnegative signed dividend always has a multiplier.
                    assert!(
                        d < 3 || d.is_power_of_two() || bound > 127,
                        "no multiplier for {d} within {bound}"
                    );
                    continue;
                };
                assert!(m < 256 && shift < 8);
                for x in 0..=bound {
                    assert_eq!(quotient(x, m, shift, 8), x / d, "{x} // {d} within {bound}");
                }
            }
        }
    }

    #[test]
    fn sixteen_bit_multipliers_divide_every_dividend_within_their_bound() {
        let divisors = (3..=300u64).chain([
            641, 1000, 1001, 10_000, 32_767, 32_769, 43_691, 65_521, 65_535,
        ]);
        for d in divisors {
            for bound in [
                d - 1,
                d,
                2 * d + 1,
                255,
                1000,
                32_767,
                32_768,
                65_534,
                65_535,
            ] {
                let bound = bound.min(65_535);
                let Some((m, shift)) = multiplier(d, bound, 16) else {
                    assert!(d.is_power_of_two() || bound > 32_767);
                    continue;
                };
                assert!(m < 65_536 && shift < 16);
                for x in 0..=bound {
                    assert_eq!(
                        quotient(x, m, shift, 16),
                        x / d,
                        "{x} // {d} within {bound}"
                    );
                }
            }
        }
    }

    /// Dividends where a wrong multiplier first fails: multiples of the
    /// divisor and their predecessors, near the bound.
    fn edges(d: u64, bound: u64) -> impl Iterator<Item = u64> {
        let top = bound / d * d;
        [
            0,
            1,
            d - 1,
            d,
            d.saturating_add(1),
            bound / 2,
            bound - 1,
            bound,
        ]
        .into_iter()
        .chain((0..4).flat_map(move |i| {
            let multiple = top.saturating_sub(d.saturating_mul(i));
            [
                multiple.saturating_sub(1),
                multiple,
                multiple.saturating_add((d - 1).min(bound - top)),
            ]
        }))
        .filter(move |x| *x <= bound)
    }

    #[test]
    fn wide_multipliers_divide_edge_dividends() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for bits in [32u32, 64] {
            let max = u64::MAX >> (64 - bits);
            let mut divisors = vec![
                3,
                5,
                6,
                7,
                10,
                641,
                1000,
                139_968,
                max / 2,
                max / 2 + 2,
                max,
            ];
            for _ in 0..20_000 {
                let d = (next() >> (next() % u64::from(bits))) & max;
                divisors.push(d.max(3));
            }
            for d in divisors {
                let bounds = [
                    d,
                    d.saturating_mul(3877) & max,
                    max >> 34.min(bits - 1),
                    max / 2,
                    max / 2 + 1,
                    max,
                    next() & max,
                    (next() & max) >> (next() % u64::from(bits)),
                ];
                for bound in bounds {
                    let bound = bound.max(1);
                    let Some((m, shift)) = multiplier(d, bound, bits) else {
                        assert!(d.is_power_of_two() || bound > max / 2, "{d} within {bound}");
                        continue;
                    };
                    assert!(u128::from(m) <= u128::from(max) && shift < bits);
                    for x in edges(d, bound) {
                        assert_eq!(
                            quotient(x, m, shift, bits),
                            x / d,
                            "{x} // {d} within {bound}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_smaller_bound_needs_a_shorter_sequence() {
        // 1000 needs a 65-bit multiplier for every u64 dividend.
        assert_eq!(multiplier(1000, u64::MAX, 64), None);
        assert_eq!(
            multiplier(1000, i64::MAX as u64, 64),
            Some((0x20c4_9ba5_e353_f7cf, 7))
        );
        // fasta's seed times its multiplier, below 2**30: no shift.
        assert_eq!(
            multiplier(139_968, 139_967 * 3877 + 29_573, 64),
            Some((0x77dd_5a03_fada, 0))
        );
        // Divisors with a one-word multiplier for every dividend keep it.
        assert_eq!(
            multiplier(10, u64::MAX, 64),
            Some((0xcccc_cccc_cccc_cccd, 3))
        );
        assert_eq!(multiplier(8, 100, 64), None);
        assert_eq!(multiplier(1, 100, 64), None);
        assert_eq!(multiplier(0, 100, 64), None);
    }
}

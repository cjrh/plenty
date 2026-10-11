//! The bare numeric cast `T(x)`. A value the target integer type cannot
//! represent traps, as arithmetic overflow does; see the numeric rules in
//! `book/src/design/language/03-types/numbers.md`.
use super::*;
use cranelift_codegen::ir::Value as NativeValue;

/// The one comparison, at the source width, that detects an integer the
/// target cannot represent.
#[derive(Debug, PartialEq)]
enum RangeCheck {
    /// Signed `value < 0`: a signed source and an unsigned target that holds
    /// the source's maximum.
    Negative,
    /// Unsigned `value > max`. Read as unsigned, a negative source value
    /// exceeds the maximum of a narrower unsigned target, so this covers
    /// both bounds of a signed source too.
    Above(i64),
    /// Signed to narrower signed: the value fits exactly when sign-extending
    /// its low bits restores it.
    SignedNarrowing,
}

/// `None` when the target represents every source value, so the cast needs
/// no check.
fn range_check(source: &Ty, target: &Ty) -> Option<RangeCheck> {
    let (source_min, source_end) = source.int_range()?;
    let (target_min, target_end) = target.int_range()?;
    let below = source_min < target_min;
    if source_end <= target_end {
        return below.then_some(RangeCheck::Negative);
    }
    if below && target_min != 0 {
        return Some(RangeCheck::SignedNarrowing);
    }
    // A maximum the source can exceed lies inside the source range, so it is
    // a constant of the source type with a clear sign bit.
    Some(RangeCheck::Above((target_end - 1) as i64))
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_checked_cast(&mut self, target: &Ty) -> Result<()> {
        let (value, source) = self.stack.pop().ok_or("AOT: stack underflow on cast")?;
        let out_of_range = if source.is_float() {
            target
                .int_range()
                .map(|(min, end)| self.float_out_of_range(value, &source, min, end))
        } else {
            range_check(&source, target).map(|check| match check {
                RangeCheck::Negative => self.bcx.ins().icmp_imm(IntCC::SignedLessThan, value, 0),
                RangeCheck::Above(max) => {
                    self.bcx
                        .ins()
                        .icmp_imm(IntCC::UnsignedGreaterThan, value, max)
                }
                RangeCheck::SignedNarrowing => {
                    let low = self.bcx.ins().ireduce(clif_type(target.clone()), value);
                    let restored = self.bcx.ins().sextend(clif_type(source.clone()), low);
                    self.bcx.ins().icmp(IntCC::NotEqual, restored, value)
                }
            })
        };
        if let Some(out_of_range) = out_of_range {
            self.trap_if(out_of_range, TrapKind::CastRange)?;
        }
        let converted = self.cast(value, source, target.clone());
        self.stack.push((converted, target.clone()));
        Ok(())
    }

    /// A float converts when its truncation toward zero lies in the target
    /// range: `target_min - 1 < value < target_end`. Both comparisons are
    /// unordered, so NaN is out of range.
    fn float_out_of_range(
        &mut self,
        value: NativeValue,
        source: &Ty,
        target_min: i128,
        target_end: i128,
    ) -> NativeValue {
        let single = *source == Ty::F32;
        let exact = |bound: i128| {
            if single {
                (bound as f32) as i128 == bound
            } else {
                (bound as f64) as i128 == bound
            }
        };
        // `target_min` is zero or a negated power of two, so it is exact in
        // both float types. Where `target_min - 1` is not, no float lies
        // strictly between the two and `value < target_min` is the same test.
        let (below, low) = if exact(target_min - 1) {
            (FloatCC::UnorderedOrLessThanOrEqual, target_min - 1)
        } else {
            (FloatCC::UnorderedOrLessThan, target_min)
        };
        let low = self.float_const(single, low);
        let high = self.float_const(single, target_end);
        let too_low = self.bcx.ins().fcmp(below, value, low);
        let too_high = self
            .bcx
            .ins()
            .fcmp(FloatCC::UnorderedOrGreaterThanOrEqual, value, high);
        self.bcx.ins().bor(too_low, too_high)
    }

    fn float_const(&mut self, single: bool, value: i128) -> NativeValue {
        if single {
            self.bcx.ins().f32const(value as f32)
        } else {
            self.bcx.ins().f64const(value as f64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEGERS: [Ty; 8] = [
        Ty::I8,
        Ty::I16,
        Ty::I32,
        Ty::I64,
        Ty::U8,
        Ty::U16,
        Ty::U32,
        Ty::U64,
    ];

    /// What the emitted comparison computes for `value` held at the source
    /// width.
    fn trips(check: &RangeCheck, value: i128, source: &Ty, target: &Ty) -> bool {
        let source_bits = u32::from(width_bits(source.clone()));
        let unsigned = |bits: u32| value & ((1i128 << bits) - 1);
        match check {
            RangeCheck::Negative => value < 0,
            RangeCheck::Above(max) => unsigned(source_bits) > i128::from(*max),
            RangeCheck::SignedNarrowing => {
                let bits = u32::from(width_bits(target.clone()));
                let low = unsigned(bits);
                let restored = if low >> (bits - 1) == 1 {
                    low - (1i128 << bits)
                } else {
                    low
                };
                restored != value
            }
        }
    }

    #[test]
    fn a_check_is_emitted_only_where_a_source_value_does_not_fit() {
        for source in &INTEGERS {
            for target in &INTEGERS {
                let (source_min, source_end) = source.int_range().unwrap();
                let (target_min, target_end) = target.int_range().unwrap();
                let check = range_check(source, target);
                assert_eq!(
                    check.is_none(),
                    target_min <= source_min && source_end <= target_end,
                    "{source} to {target}"
                );
                let Some(check) = check else { continue };
                let mut values = vec![
                    source_min,
                    source_min + 1,
                    -1,
                    0,
                    1,
                    source_end - 2,
                    source_end - 1,
                ];
                for bound in [target_min, target_end] {
                    values.extend([bound - 2, bound - 1, bound, bound + 1]);
                }
                for value in values {
                    if !(source_min..source_end).contains(&value) {
                        continue;
                    }
                    assert_eq!(
                        trips(&check, value, source, target),
                        !(target_min..target_end).contains(&value),
                        "{value}{source} to {target} with {check:?}"
                    );
                }
            }
        }
    }
}

//! `checked:` blocks: integer `+ - * // %`, unary `-`, `pow` and shifts
//! inside one exit the block with an `Err` holding `OverflowError[T]`,
//! `ZeroDivisionError` or `ShiftCountError[T]` instead of trapping; float `div_checked` is a block
//! holding one float division. See
//! `book/src/design/language/03-types/numbers.md#checked-blocks`.
//!
//! Each operation computes its wrapped value and failure flags without
//! branching, then branches on each flag that is not known to be false to a
//! cold block. That block builds the error, releases the block's
//! temporaries and locals, and jumps to the block's exit with the `Err`.
//! Explicit blocks wrap the successful value in `Ok`; function boundaries
//! pass the declared Result through unchanged. Both join the same cold exits.
use super::*;
use crate::numeric::Checked;
use crate::sum::EnumOp;
use cranelift_codegen::ir::Value as NativeValue;

/// The innermost local or function boundary that failing arithmetic exits.
pub(super) struct Target {
    /// Takes the block's `Result` as its parameter.
    exit: Block,
    /// The stack depth at the block's start; a failure releases the entries
    /// above it.
    base: usize,
    /// The block's `Result[T, E]`.
    result: Ty,
    /// The block is the body of a function lowered into its caller, whose
    /// `?` may bypass `exit`; see [`super::known_returns`].
    inlined: bool,
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_checked_block(
        &mut self,
        body: &[Op],
        result: &Ty,
        function: bool,
    ) -> Result<()> {
        let Ty::Enum(sum) = result else {
            return Err("AOT: a checked block must produce a Result".into());
        };
        let base = self.stack.len();
        let inlined = function
            && self
                .exits
                .as_ref()
                .is_some_and(|exits| exits.returns_to.is_some());
        let exit = self.bcx.create_block();
        self.bcx.append_block_param(exit, clif_type(result.clone()));
        self.checked_targets.push(Target {
            exit,
            inlined,
            base: self.stack.len(),
            result: result.clone(),
        });
        for op in body {
            if self.terminated {
                break;
            }
            self.lower(op)?;
        }
        self.checked_targets.pop();
        if !self.terminated {
            if !function {
                if sum.get().variants[0].fields[0] == Ty::Unit {
                    self.lower(&Op::PushUnit)?;
                }
                self.lower_enum(&EnumOp::New(sum.clone(), 0))?;
            }
            let (ok, _) = self.stack.pop().ok_or("AOT: missing checked block value")?;
            let jump = self.bcx.ins().jump(exit, &[BlockArg::Value(ok)]);
            if inlined {
                self.note_join(exit, jump);
            }
        }
        self.bcx.switch_to_block(exit);
        // The caller's `?` may move these jumps on to its own blocks.
        self.seal_join_later(exit, inlined);
        let value = self.bcx.block_params(exit)[0];
        // An explicit return terminates its path with return values still on
        // the lowering stack. Only the block's entry operands dominate this join.
        self.stack.truncate(base);
        self.stack.push((value, result.clone()));
        self.terminated = false;
        Ok(())
    }

    pub(super) fn lower_block_arith(
        &mut self,
        operation: Checked,
        ty: &Ty,
        cleanup: &[Op],
    ) -> Result<()> {
        let (b, _) = self.stack.pop().ok_or("AOT: stack underflow")?;
        let (a, _) = self.stack.pop().ok_or("AOT: stack underflow")?;
        let (value, failures) = match operation {
            Checked::Add | Checked::Sub | Checked::Mul => {
                let kind = match operation {
                    Checked::Add => ArithKind::Add,
                    Checked::Sub => ArithKind::Sub,
                    _ => ArithKind::Mul,
                };
                let (value, overflow) = self.overflowing(kind, a, b, ty);
                // Past the overflow exit, nonnegative operands give a
                // nonnegative sum or product.
                if super::is_signed(ty.clone())
                    && operation != Checked::Sub
                    && self.nonnegative_operand(a)
                    && self.nonnegative_operand(b)
                {
                    self.counters.nonnegative.insert(value);
                }
                (value, vec![(overflow, TrapKind::Overflow)])
            }
            Checked::FloorDiv(proof) => self.checked_floor_div(a, b, ty, proof),
            Checked::Modulo(proof) => self.checked_modulo(a, b, ty, proof),
            Checked::Pow => {
                let (value, overflow) = self.int_pow_checked(a, b, ty)?;
                (value, vec![(overflow, TrapKind::Overflow)])
            }
            Checked::ShiftLeft | Checked::ShiftRight => {
                // Unsigned comparison rejects negative signed counts as well.
                let width = clif_type(ty.clone()).bits();
                let invalid = self.bcx.ins().icmp_imm_s(
                    IntCC::UnsignedGreaterThanOrEqual,
                    b,
                    i64::from(width),
                );
                // Cranelift masks the count; the result is unused on failure.
                let value = match operation {
                    Checked::ShiftLeft => self.bcx.ins().ishl(a, b),
                    _ if is_signed(ty.clone()) => self.bcx.ins().sshr(a, b),
                    _ => self.bcx.ins().ushr(a, b),
                };
                (value, vec![(invalid, TrapKind::Shift)])
            }
            Checked::FloatDiv => {
                let failures = if known::known_float(&self.bcx.func.dfg, b)
                    .is_some_and(|divisor| divisor != 0.0)
                {
                    Vec::new()
                } else {
                    let zero = if *ty == Ty::F32 {
                        self.bcx.ins().f32const(0.0)
                    } else {
                        self.bcx.ins().f64const(0.0)
                    };
                    // Equality holds for both signed zeros.
                    let flag = self.bcx.ins().fcmp(FloatCC::Equal, b, zero);
                    vec![(flag, TrapKind::DivZero)]
                };
                let a = self.float_left_operand(a, b);
                (self.bcx.ins().fdiv(a, b), failures)
            }
        };
        // A shift error reports its count; the others the wrapped result.
        let reported = if operation.shifts() { b } else { value };
        for (flag, kind) in failures {
            if known::known_int(&self.bcx.func.dfg, flag) == Some(0) {
                continue;
            }
            self.exit_checked_if(flag, kind, reported, ty, cleanup)?;
        }
        self.stack.push((value, ty.clone()));
        Ok(())
    }

    /// Branch on `flag` to a cold block that exits the innermost `checked:`
    /// block with the error for `kind`.
    fn exit_checked_if(
        &mut self,
        flag: NativeValue,
        kind: TrapKind,
        wrapped: NativeValue,
        ty: &Ty,
        cleanup: &[Op],
    ) -> Result<()> {
        let target = self
            .checked_targets
            .last()
            .ok_or("AOT: checked arithmetic outside a checked block")?;
        let (exit, base, result) = (target.exit, target.base, target.result.clone());
        let inlined = target.inlined;
        let Ty::Enum(sum) = &result else {
            unreachable!("checked block results are Result enums")
        };
        let before = self.bcx.current_block();
        let failure = self.bcx.create_block();
        let success = self.bcx.create_block();
        self.bcx.set_cold_block(failure);
        self.bcx.ins().brif(flag, failure, &[], success, &[]);
        self.bcx.switch_to_block(failure);
        self.bcx.seal_block(failure);
        let pending = self.stack.clone();
        let error = sum.get().variants[1].fields[0].clone();
        let member = self.arithmetic_error(kind, wrapped, ty, &error)?;
        self.stack.push(member);
        self.lower_enum(&EnumOp::New(sum.clone(), 1))?;
        let (err, _) = self.stack.pop().ok_or("AOT: missing checked error")?;
        while self.stack.len() > base {
            let (value, ty) = self.stack.pop().unwrap();
            self.release(value, &ty);
        }
        for op in cleanup {
            self.lower(op)?;
        }
        let jump = self.bcx.ins().jump(exit, &[BlockArg::Value(err)]);
        if inlined {
            self.note_join(exit, jump);
        }
        self.stack = pending;
        // Only `before` reaches `success`, so its row checks still hold
        // there (issue #721), as after a trap guard.
        self.bcx.switch_to_block(success);
        self.bcx.seal_block(success);
        self.counters.checked.continue_to(before, success);
        Ok(())
    }

    /// The error a failure of `kind` reports, as a value of `error`: an
    /// `OverflowError[T]` holding `wrapped`, a `ShiftCountError[T]` holding
    /// the count passed as `wrapped`, or a `ZeroDivisionError`, widened into
    /// `error` when it is a union.
    fn arithmetic_error(
        &mut self,
        kind: TrapKind,
        wrapped: NativeValue,
        ty: &Ty,
        error: &Ty,
    ) -> Result<(NativeValue, Ty)> {
        // A generic instance's name carries its arguments.
        let (name, field) = match kind {
            TrapKind::Overflow => ("OverflowError[", format!("OverflowError[{ty}]")),
            TrapKind::Shift => ("ShiftCountError[", format!("ShiftCountError[{ty}]")),
            _ => ("ZeroDivisionError", "ZeroDivisionError".to_string()),
        };
        let member = crate::sum::members(error)
            .into_iter()
            .find(|member| {
                matches!(member, Ty::Class(class)
                    if class.name.starts_with(name) && class.name.ends_with(&field[name.len()..]))
            })
            .ok_or_else(|| format!("AOT: {error} does not include {field}"))?;
        // `OverflowError[T]` and `ShiftCountError[T]` are newtypes,
        // represented as their field; `ZeroDivisionError` has no fields and
        // occupies no storage.
        let value = if matches!(kind, TrapKind::Overflow | TrapKind::Shift) {
            self.pack(wrapped, ty)
        } else {
            let zero = self.bcx.ins().iconst(types::I64, 0);
            self.bcx.ins().uextend(types::I128, zero)
        };
        if member == *error {
            return Ok((value, member));
        }
        self.stack.push((value, member.clone()));
        self.lower_enum(&EnumOp::Widen(Box::new(member), Box::new(error.clone())))?;
        self.stack
            .pop()
            .ok_or_else(|| "AOT: missing widened error".into())
    }

    /// The wrapped sum, difference or product and its overflow flag.
    fn overflowing(
        &mut self,
        kind: ArithKind,
        a: NativeValue,
        b: NativeValue,
        ty: &Ty,
    ) -> (NativeValue, NativeValue) {
        let signed = is_signed(ty.clone());
        if signed && let Some(value) = self.lower_bounded_arith(kind, a, b) {
            return (value, self.bcx.ins().iconst(types::I8, 0));
        }
        match self.compared_add_sub(kind, signed, a, b) {
            Some(checked) => checked,
            None => match (kind, signed) {
                (ArithKind::Add, true) => self.bcx.ins().sadd_overflow(a, b),
                (ArithKind::Add, false) => self.bcx.ins().uadd_overflow(a, b),
                (ArithKind::Sub, true) => self.bcx.ins().ssub_overflow(a, b),
                (ArithKind::Sub, false) => self.bcx.ins().usub_overflow(a, b),
                (ArithKind::Mul, true) => self.bcx.ins().smul_overflow(a, b),
                (ArithKind::Mul, false) => self.bcx.ins().umul_overflow(a, b),
            },
        }
    }

    /// The divisor's zero flag and, for a signed divisor that may be `-1`,
    /// that flag too. Both are absent for a constant divisor they exclude.
    fn divisor_flags(
        &mut self,
        a: NativeValue,
        b: NativeValue,
        ty: &Ty,
        proof: crate::numeric::Divisor,
    ) -> (Option<NativeValue>, Option<NativeValue>) {
        let signed = proof.may_be_minus_one()
            && is_signed(ty.clone())
            && !self.nonnegative_division(a, b, proof);
        if let Some(divisor) = known::known_signed(&self.bcx.func.dfg, b) {
            let zero =
                (proof.may_be_zero() && divisor == 0).then(|| self.bcx.ins().iconst(types::I8, 1));
            let minus_one = (signed && divisor == -1).then(|| self.bcx.ins().iconst(types::I8, 1));
            return (zero, minus_one);
        }
        let zero = proof
            .may_be_zero()
            .then(|| self.bcx.ins().icmp_imm_s(IntCC::Equal, b, 0));
        let minus_one = signed.then(|| self.bcx.ins().icmp_imm_s(IntCC::Equal, b, -1));
        (zero, minus_one)
    }

    /// `b`, or 1 where any of `flags` holds, so the hardware division cannot
    /// fault.
    fn safe_divisor(
        &mut self,
        b: NativeValue,
        ty: &Ty,
        flags: &[Option<NativeValue>],
    ) -> NativeValue {
        let mut unsafe_divisor = None;
        for flag in flags.iter().flatten() {
            unsafe_divisor = Some(match unsafe_divisor {
                Some(earlier) => self.bcx.ins().bor(earlier, *flag),
                None => *flag,
            });
        }
        match unsafe_divisor {
            Some(flag) => {
                let one = self.bcx.ins().iconst(clif_type(ty.clone()), 1);
                self.bcx.ins().select(flag, one, b)
            }
            None => b,
        }
    }

    /// Floor division. `MIN // -1` wraps to `MIN`, the negation of the
    /// dividend modulo 2**N.
    fn checked_floor_div(
        &mut self,
        a: NativeValue,
        b: NativeValue,
        ty: &Ty,
        proof: crate::numeric::Divisor,
    ) -> (NativeValue, Vec<(NativeValue, TrapKind)>) {
        let positive = proof == crate::numeric::Divisor::Positive;
        let (zero, minus_one) = self.divisor_flags(a, b, ty, proof);
        let divisor = self.safe_divisor(b, ty, &[zero, minus_one]);
        let mut failures: Vec<_> = zero
            .map(|flag| (flag, TrapKind::DivZero))
            .into_iter()
            .collect();
        let Some(minus_one) = minus_one else {
            let value = if is_signed(ty.clone()) && !self.nonnegative_division(a, b, proof) {
                self.floored(a, divisor, positive)
            } else {
                self.bcx.ins().udiv(a, divisor)
            };
            return (value, failures);
        };
        let quotient = self.floored(a, divisor, positive);
        let negated = self.bcx.ins().ineg(a);
        let value = self.bcx.ins().select(minus_one, negated, quotient);
        let int_min = match ty {
            Ty::I8 => i64::from(i8::MIN),
            Ty::I16 => i64::from(i16::MIN),
            Ty::I32 => i64::from(i32::MIN),
            _ => i64::MIN,
        };
        let is_min = self.bcx.ins().icmp_imm_s(IntCC::Equal, a, int_min);
        let overflow = self.bcx.ins().band(is_min, minus_one);
        failures.push((overflow, TrapKind::Overflow));
        (value, failures)
    }

    /// Signed division rounded toward negative infinity, by a divisor that
    /// is neither zero nor `-1`. `positive` states that the divisor is
    /// greater than zero.
    ///
    /// An arithmetic shift already rounds toward negative infinity, so a
    /// power-of-two divisor needs no division. For any positive divisor the
    /// truncated quotient is one too high exactly when the remainder is
    /// negative, and the remainder's sign word is that `-1` or `0`.
    pub(super) fn floored(
        &mut self,
        a: NativeValue,
        divisor: NativeValue,
        positive: bool,
    ) -> NativeValue {
        let dfg = &self.bcx.func.dfg;
        let sign_shift = i64::from(dfg.value_type(a).bits()) - 1;
        let constant = known::known_signed(dfg, divisor).filter(|c| *c > 0);
        if let Some(power) = constant.filter(|c| c.count_ones() == 1) {
            return self
                .bcx
                .ins()
                .sshr_imm_u(a, i64::from(power.trailing_zeros()));
        }
        let quotient = self.bcx.ins().sdiv(a, divisor);
        let remainder = self.bcx.ins().srem(a, divisor);
        if positive || constant.is_some() {
            let sign = self.bcx.ins().sshr_imm_u(remainder, sign_shift);
            return self.bcx.ins().iadd(quotient, sign);
        }
        let nonzero = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, remainder, 0);
        let signs = self.bcx.ins().bxor(a, divisor);
        let different = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
        let round_down = self.bcx.ins().band(nonzero, different);
        let below = self.bcx.ins().iadd_imm_s(quotient, -1);
        self.bcx.ins().select(round_down, below, quotient)
    }

    /// Signed remainder with the sign of `b`, dividing by `divisor`: `b`, or
    /// its replacement where `b` is `-1`. `positive` states that `b` is
    /// greater than zero.
    ///
    /// A power-of-two modulus keeps the low bits. For any positive modulus
    /// the truncated remainder is too low by `b` exactly when it is negative.
    pub(super) fn floored_remainder(
        &mut self,
        a: NativeValue,
        divisor: NativeValue,
        b: NativeValue,
        positive: bool,
    ) -> NativeValue {
        let dfg = &self.bcx.func.dfg;
        let sign_shift = i64::from(dfg.value_type(a).bits()) - 1;
        let constant = known::known_signed(dfg, b).filter(|c| *c > 0);
        if let Some(power) = constant.filter(|c| c.count_ones() == 1) {
            return self.bcx.ins().band_imm_s(a, power - 1);
        }
        let remainder = self.bcx.ins().srem(a, divisor);
        if positive || constant.is_some() {
            let sign = self.bcx.ins().sshr_imm_u(remainder, sign_shift);
            let correction = self.bcx.ins().band(sign, b);
            return self.bcx.ins().iadd(remainder, correction);
        }
        let nonzero = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, remainder, 0);
        let signs = self.bcx.ins().bxor(remainder, b);
        let different = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
        let adjust = self.bcx.ins().band(nonzero, different);
        let corrected = self.bcx.ins().iadd(remainder, b);
        self.bcx.ins().select(adjust, corrected, remainder)
    }

    /// Modulo with the divisor's sign. `MIN % -1` is 0, which a divisor of
    /// 1 also gives, so `-1` is not a failure.
    fn checked_modulo(
        &mut self,
        a: NativeValue,
        b: NativeValue,
        ty: &Ty,
        proof: crate::numeric::Divisor,
    ) -> (NativeValue, Vec<(NativeValue, TrapKind)>) {
        let (zero, minus_one) = self.divisor_flags(a, b, ty, proof);
        let divisor = self.safe_divisor(b, ty, &[zero, minus_one]);
        let failures: Vec<_> = zero
            .map(|flag| (flag, TrapKind::DivZero))
            .into_iter()
            .collect();
        let value = if is_signed(ty.clone()) && !self.nonnegative_division(a, b, proof) {
            let positive = proof == crate::numeric::Divisor::Positive;
            self.floored_remainder(a, divisor, b, positive)
        } else {
            self.bcx.ins().urem(a, divisor)
        };
        self.note_remainder(value, b, ty, proof);
        (value, failures)
    }
}

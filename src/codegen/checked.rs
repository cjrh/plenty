//! `checked:` blocks: integer `+ - * // %`, unary `-` and `pow` inside one
//! exit the block with an `Err` holding `OverflowError[T]` or
//! `ZeroDivisionError` instead of trapping; float `div_checked` is a block
//! holding one float division. See
//! `book/src/design/language/03-types/numbers.md#checked-blocks`.
//!
//! Each operation computes its wrapped value and failure flags without
//! branching, then branches on each flag that is not known to be false to a
//! cold block. That block builds the error, releases the block's
//! temporaries and locals, and jumps to the block's exit with the `Err`.
//! The normal path wraps the body's value in `Ok` and jumps to the same exit.
use super::*;
use crate::numeric::Checked;
use crate::sum::EnumOp;
use cranelift_codegen::ir::Value as NativeValue;

/// The innermost `checked:` block that failing arithmetic exits.
pub(super) struct Target {
    /// Takes the block's `Result` as its parameter.
    exit: Block,
    /// The stack depth at the block's start; a failure releases the entries
    /// above it.
    base: usize,
    /// The block's `Result[T, E]`.
    result: Ty,
}

impl Lowerer<'_, '_> {
    pub(super) fn lower_checked_block(&mut self, body: &[Op], result: &Ty) -> Result<()> {
        let Ty::Enum(sum) = result else {
            return Err("AOT: a checked block must produce a Result".into());
        };
        let exit = self.bcx.create_block();
        self.bcx.append_block_param(exit, clif_type(result.clone()));
        self.checked_targets.push(Target {
            exit,
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
            self.lower_enum(&EnumOp::New(sum.clone(), 0))?;
            let (ok, _) = self.stack.pop().ok_or("AOT: missing checked block value")?;
            self.bcx.ins().jump(exit, &[BlockArg::Value(ok)]);
        }
        self.bcx.switch_to_block(exit);
        self.bcx.seal_block(exit);
        let value = self.bcx.block_params(exit)[0];
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
                (value, vec![(overflow, TrapKind::Overflow)])
            }
            Checked::FloorDiv => self.checked_floor_div(a, b, ty),
            Checked::Modulo => self.checked_modulo(a, b, ty),
            Checked::Pow => {
                let (value, overflow) = self.int_pow_checked(a, b, ty)?;
                (value, vec![(overflow, TrapKind::Overflow)])
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
        for (flag, kind) in failures {
            if known::known_int(&self.bcx.func.dfg, flag) == Some(0) {
                continue;
            }
            self.exit_checked_if(flag, kind, value, ty, cleanup)?;
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
        let Ty::Enum(sum) = &result else {
            unreachable!("checked block results are Result enums")
        };
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
        self.bcx.ins().jump(exit, &[BlockArg::Value(err)]);
        self.stack = pending;
        self.bcx.switch_to_block(success);
        self.bcx.seal_block(success);
        Ok(())
    }

    /// The error a failure of `kind` reports, as a value of `error`: an
    /// `OverflowError[T]` holding `wrapped`, or a `ZeroDivisionError`,
    /// widened into `error` when it is a union.
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
            _ => ("ZeroDivisionError", "ZeroDivisionError".to_string()),
        };
        let member = crate::sum::members(error)
            .into_iter()
            .find(|member| {
                matches!(member, Ty::Class(class)
                    if class.name.starts_with(name) && class.name.ends_with(&field[name.len()..]))
            })
            .ok_or_else(|| format!("AOT: {error} does not include {field}"))?;
        // `OverflowError[T]` is a newtype, represented as its field;
        // `ZeroDivisionError` has no fields and occupies no storage.
        let value = if kind == TrapKind::Overflow {
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
    ) -> (Option<NativeValue>, Option<NativeValue>) {
        let signed = is_signed(ty.clone()) && !self.nonnegative_operands(a, b);
        if let Some(divisor) = known::known_signed(&self.bcx.func.dfg, b) {
            let zero = (divisor == 0).then(|| self.bcx.ins().iconst(types::I8, 1));
            let minus_one = (signed && divisor == -1).then(|| self.bcx.ins().iconst(types::I8, 1));
            return (zero, minus_one);
        }
        let zero = self.bcx.ins().icmp_imm_s(IntCC::Equal, b, 0);
        let minus_one = signed.then(|| self.bcx.ins().icmp_imm_s(IntCC::Equal, b, -1));
        (Some(zero), minus_one)
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
    ) -> (NativeValue, Vec<(NativeValue, TrapKind)>) {
        let (zero, minus_one) = self.divisor_flags(a, b, ty);
        let divisor = self.safe_divisor(b, ty, &[zero, minus_one]);
        let mut failures: Vec<_> = zero
            .map(|flag| (flag, TrapKind::DivZero))
            .into_iter()
            .collect();
        let Some(minus_one) = minus_one else {
            let value = if is_signed(ty.clone()) && !self.nonnegative_operands(a, b) {
                self.floored(a, divisor)
            } else {
                self.bcx.ins().udiv(a, divisor)
            };
            return (value, failures);
        };
        let quotient = self.floored(a, divisor);
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
    /// is neither zero nor `-1`.
    fn floored(&mut self, a: NativeValue, divisor: NativeValue) -> NativeValue {
        let quotient = self.bcx.ins().sdiv(a, divisor);
        let remainder = self.bcx.ins().srem(a, divisor);
        let nonzero = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, remainder, 0);
        let signs = self.bcx.ins().bxor(a, divisor);
        let different = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
        let round_down = self.bcx.ins().band(nonzero, different);
        let below = self.bcx.ins().iadd_imm_s(quotient, -1);
        self.bcx.ins().select(round_down, below, quotient)
    }

    /// Modulo with the divisor's sign. `MIN % -1` is 0, which a divisor of
    /// 1 also gives, so `-1` is not a failure.
    fn checked_modulo(
        &mut self,
        a: NativeValue,
        b: NativeValue,
        ty: &Ty,
    ) -> (NativeValue, Vec<(NativeValue, TrapKind)>) {
        let (zero, minus_one) = self.divisor_flags(a, b, ty);
        let divisor = self.safe_divisor(b, ty, &[zero, minus_one]);
        let failures: Vec<_> = zero
            .map(|flag| (flag, TrapKind::DivZero))
            .into_iter()
            .collect();
        let value = if is_signed(ty.clone()) && !self.nonnegative_operands(a, b) {
            let remainder = self.bcx.ins().srem(a, divisor);
            let nonzero = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, remainder, 0);
            let signs = self.bcx.ins().bxor(remainder, divisor);
            let different = self.bcx.ins().icmp_imm_s(IntCC::SignedLessThan, signs, 0);
            let adjust = self.bcx.ins().band(nonzero, different);
            let corrected = self.bcx.ins().iadd(remainder, divisor);
            self.bcx.ins().select(adjust, corrected, remainder)
        } else {
            self.bcx.ins().urem(a, divisor)
        };
        (value, failures)
    }
}

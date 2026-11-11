//! Numeric conversions written with a type name: the bare cast `T(x)`, which
//! requires the value to be representable in `T`, and `T.wrapping_from(x)`,
//! which keeps an integer's low bits. `T.try_from` is the recoverable form.
use super::*;

impl Lower<'_> {
    /// Emit the bare cast at `cast` of `operand`, which has just been lowered.
    /// A literal operand is rejected here when it does not fit; any other
    /// operand is checked when the program runs.
    pub(super) fn checked_cast(
        &mut self,
        source: &Ty,
        target: &Ty,
        operand: &Expr,
        cast: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let at = &operand.at;
        // A literal lowers to one push. Other expressions can end in a push
        // that only some paths reach, so the operand's syntax decides.
        let literal = match &operand.kind {
            Expression::Number(_) => true,
            Expression::Unary(sign, value) => {
                sign == "-" && matches!(value.kind, Expression::Number(_))
            }
            _ => false,
        };
        if !literal
            && let Some(interval) = self.operand_interval(operand, source)
            && let (interval, true) = interval.cast_or_full(target)
        {
            // Every value of the operand's interval is a value of `target`:
            // the conversion cannot fail and has no run-time check.
            self.mark(cast, ops);
            ops.push(Op::Proved(
                Box::new(Op::CheckedCast(target.clone())),
                interval,
            ));
            self.note_interval(intervals::node(cast), target, interval);
            return Ok(());
        }
        if !literal {
            // A literal cast is checked when compiling, so only this one can
            // trap inside a `checked:` block.
            if let Some(scope) = self.checked.last_mut()
                && scope.arithmetic
                && !scope.function
            {
                scope.casts = true;
            }
            self.mark(cast, ops);
            self.push_trapping(Op::CheckedCast(target.clone()), target, ops);
            return Ok(());
        }
        if let Some((min, end)) = target.int_range() {
            match ops.last() {
                Some(Op::PushInt(value)) if source.is_int() => {
                    let number = match *value {
                        Value::I8(n) => i128::from(n),
                        Value::I16(n) => i128::from(n),
                        Value::I32(n) => i128::from(n),
                        Value::I64(n) => i128::from(n),
                        Value::U8(n) => i128::from(n),
                        Value::U16(n) => i128::from(n),
                        Value::U32(n) => i128::from(n),
                        Value::U64(n) => i128::from(n),
                    };
                    if !(min..end).contains(&number) {
                        return Err(at.error(format!(
                            "{number} is out of range for {target}; {target}.wrapping_from keeps the low bits"
                        )));
                    }
                    self.push_trapping(Op::CheckedCast(target.clone()), target, ops);
                    let point = crate::interval::Interval::point(number);
                    self.note_interval(intervals::node(cast), target, point);
                    return Ok(());
                }
                Some(Op::PushFloat { bits, ty }) if source.is_float() => {
                    let (number, text) = if *ty == Ty::F32 {
                        let number = f32::from_bits(*bits as u32);
                        (f64::from(number), format!("{number:?}"))
                    } else {
                        let number = f64::from_bits(*bits);
                        (number, format!("{number:?}"))
                    };
                    // Both bounds are zero or powers of two, so they are exact.
                    let whole = number.trunc();
                    if !(whole >= min as f64 && whole < end as f64) {
                        return Err(at.error(format!("{text} is out of range for {target}")));
                    }
                }
                _ => {}
            }
        }
        self.push_trapping(Op::CheckedCast(target.clone()), target, ops);
        Ok(())
    }

    pub(super) fn wrapping_from(
        &mut self,
        ty: Ty,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if ty.is_float() {
            return Err(
                at.error("wrapping_from is defined for integer types; use a cast for floats")
            );
        }
        if !ty.is_int() {
            return Err(at.error("wrapping_from is defined for integer types"));
        }
        if args.len() != 1 {
            return Err(at.error("wrapping_from takes one integer argument"));
        }
        let source = self.value(&args[0], ops)?;
        if !source.is_int() {
            return Err(operand_error(
                "wrapping_from requires an integer value",
                &source,
                Ty::is_int,
                &args[0],
                &args[0].at,
            ));
        }
        ops.push(Op::Cast(ty.clone()));
        Ok(ty)
    }
}

/// The diagnostic for a conversion operand that `accepts` rejects. Borrowed
/// iteration yields references (`for byte in &buffer` gives `&u8`), so a
/// reference to an accepted value names the missing `*`.
pub(super) fn operand_error(
    message: &str,
    source: &Ty,
    accepts: fn(&Ty) -> bool,
    operand: &Expr,
    at: &Token,
) -> Box<dyn Error> {
    match source {
        Ty::Ref(target, _) if accepts(target) => {
            let mut operand = operand;
            while let Expression::Group(inner) = &operand.kind {
                operand = inner;
            }
            let read = match &operand.kind {
                Expression::Name(name) => format!("read it with `*{name}`"),
                Expression::Unary(op, _) if op == "&" || op == "&mut" => {
                    format!("remove the `{op}`")
                }
                _ => "read the value with `*`".to_string(),
            };
            at.error(format!("{message}, not a reference; {read}"))
        }
        _ => at.error(message),
    }
}

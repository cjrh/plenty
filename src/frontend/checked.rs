//! `checked:` blocks. Integer `+ - * // %`, unary `-` and `pow` written
//! inside one exit the block with `Err` instead of trapping; the block's
//! value is `Result[T, E]`, where `E` holds the failures its arithmetic can
//! produce. The rule is lexical: calls inside the block keep trapping. See
//! `book/src/design/language/03-types/numbers.md#checked-blocks`.
use super::*;
use crate::numeric::Checked;

/// One enclosing `checked:` block.
pub(super) struct Scope {
    /// The first local the block declares; a failure drops it and every
    /// later local.
    start: usize,
    /// The errors the block's arithmetic can produce, in source order.
    failures: Vec<Ty>,
}

impl Lower<'_> {
    pub(super) fn checked_value(
        &mut self,
        at: &Token,
        body: &[Stmt],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let declared = match &expected {
            Some(Ty::Enum(result)) if result.is_result() => Some(result.clone()),
            _ => None,
        };
        let success = declared
            .as_ref()
            .map(|result| result.get().variants[0].fields[0].clone())
            .filter(|ty| *ty != Ty::Unit);
        let saved = self.names.clone();
        self.checked.push(Scope {
            start: self.locals.len(),
            failures: Vec::new(),
        });
        let mut inner = Vec::new();
        let outer = std::mem::replace(&mut self.checked_value_tail, true);
        let result = self.block(body, &mut inner, &Tail::Value(success));
        self.checked_value_tail = outer;
        self.names = saved;

        let scope = self.checked.pop().expect("pushed above");
        let ty = match result? {
            BlockResult::Continues(ty) => ty.unwrap_or(Ty::Unit),
            BlockResult::Exits => {
                return Err(at.error(
                    "every path through this `checked:` block exits, so it has no value; remove `checked:`",
                ));
            }
        };
        if scope.failures.is_empty() {
            return Err(at.error(
                "this `checked:` block contains no arithmetic that can fail; remove `checked:`, or move the arithmetic it should check into it",
            ));
        }
        let error = crate::sum::anonymous(scope.failures);
        // A declared error that includes every possible failure is kept, so
        // `r: Result[i64, OverflowError[i64] | ParseError] = checked: ...`
        // needs no conversion.
        let result = match declared {
            Some(result)
                if result.get().variants[0].fields[0] == ty
                    && crate::sum::accepts(&result.get().variants[1].fields[0], &error) =>
            {
                Ty::Enum(result)
            }
            _ => crate::sum::result(ty, error),
        };
        ops.push(Op::CheckedBlock {
            body: inner.into(),
            result: result.clone(),
        });
        Ok(Some(result))
    }

    /// Push `operation` on two operands of type `ty` if a `checked:` block
    /// encloses it, recording the errors it can produce. False outside a
    /// block, where the caller pushes the trapping operator.
    pub(super) fn checked_arith(
        &mut self,
        operation: Checked,
        ty: &Ty,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<bool> {
        let Some(scope) = self.checked.last() else {
            return Ok(false);
        };
        let start = scope.start;
        let mut failures = Vec::new();
        if operation.divides() {
            failures.push(self.arithmetic_error("ZeroDivisionError", None, at)?);
        }
        if operation.overflows(ty) {
            failures.push(self.arithmetic_error("OverflowError", Some(ty), at)?);
        }
        let mut cleanup = Vec::new();
        self.cleanup(start, &mut cleanup);
        let scope = self.checked.last_mut().expect("checked above");
        for failure in failures {
            if !scope.failures.contains(&failure) {
                scope.failures.push(failure);
            }
        }
        ops.push(Op::CheckedArith {
            operation,
            ty: ty.clone(),
            cleanup: cleanup.into(),
        });
        Ok(true)
    }

    /// `ZeroDivisionError`, or `OverflowError[T]` for `operand` `T`.
    fn arithmetic_error(&self, name: &str, operand: Option<&Ty>, at: &Token) -> Result<Ty> {
        let args = operand
            .map(|ty| TypeRef {
                concrete: Some(Box::new(ty.clone())),
                parameter: false,
                at: at.clone(),
                name: None,
                args: Vec::new(),
            })
            .into_iter()
            .collect();
        TypeRef {
            concrete: None,
            parameter: false,
            at: at.clone(),
            name: Some(name.into()),
            args,
        }
        .resolve(self.aliases)?
        .ok_or_else(|| at.error("arithmetic error type is missing"))
    }

    /// `a.div_checked(b)` on floats, with `a` already lowered and `operand`
    /// pushing `b`: a `checked:` block holding the division, so
    /// `Result[F, ZeroDivisionError]` wherever it is written.
    pub(super) fn float_div_checked(
        &mut self,
        ty: &Ty,
        at: &Token,
        operand: Vec<Op>,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let error = self.arithmetic_error("ZeroDivisionError", None, at)?;
        let result = crate::sum::result(ty.clone(), error);
        let mut body = operand;
        body.push(Op::CheckedArith {
            operation: Checked::FloatDiv,
            ty: ty.clone(),
            cleanup: Arc::from([]),
        });
        ops.push(Op::CheckedBlock {
            body: body.into(),
            result: result.clone(),
        });
        Ok(result)
    }
}

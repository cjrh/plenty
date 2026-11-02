//! `checked:` blocks. Integer `+ - * // %`, unary `-` and `pow` written
//! inside one exit the block with `Err` instead of trapping; the block's
//! value is `Result[T, E]`, where `E` holds the failures its arithmetic can
//! produce. Closure bodies defined there inherit checking; other calls keep
//! their signatures. A private Result ABI carries lexical callback failures. See
//! `book/src/design/language/03-types/numbers.md#checked-blocks`.
use super::*;
use crate::numeric::Checked;

/// One enclosing `checked:` block.
pub(super) struct Scope {
    /// The first local the block declares; a failure drops it and every
    /// later local.
    pub(super) start: usize,
    /// A borrower forwards failures but must not check its own arithmetic.
    pub(super) arithmetic: bool,
    /// The errors the block's arithmetic can produce, in source order.
    pub(super) failures: Vec<Ty>,
}

pub(super) fn input_errors(inputs: &[(String, Ty)]) -> Vec<Ty> {
    fn collect(ty: &Ty, failures: &mut Vec<Ty>) {
        match ty {
            Ty::Ref(inner, _) => collect(inner, failures),
            Ty::Closure(closure) => {
                if let Some(error) = &closure.checked_error {
                    for failure in crate::sum::members(error) {
                        if !failures.contains(&failure) {
                            failures.push(failure);
                        }
                    }
                }
            }
            Ty::Enum(sum) if sum.get().restricted_storage => {
                for field in sum
                    .get()
                    .variants
                    .iter()
                    .flat_map(|variant| &variant.fields)
                {
                    collect(field, failures);
                }
            }
            _ => {}
        }
    }
    let mut failures = Vec::new();
    for (_, ty) in inputs {
        collect(ty, &mut failures);
    }
    failures
}

impl Lower<'_> {
    pub(super) fn rebase_checked(&mut self, ops: &mut [Op], at: &Token) -> Result<()> {
        fn rewrite(ops: &mut [Op], cleanup: &Arc<[Op]>, failures: &mut Vec<Ty>) {
            for op in ops {
                match op {
                    Op::CheckedUnwrap {
                        result: Ty::Enum(result),
                        cleanup: target,
                    } => {
                        *target = cleanup.clone();
                        for error in crate::sum::members(&result.get().variants[1].fields[0]) {
                            if !failures.contains(&error) {
                                failures.push(error);
                            }
                        }
                    }
                    Op::Loop { condition, body } => {
                        rewrite(Arc::make_mut(condition), cleanup, failures);
                        rewrite(Arc::make_mut(body), cleanup, failures);
                    }
                    Op::Match(arms) => {
                        for arm in Arc::make_mut(arms) {
                            rewrite(Arc::make_mut(&mut arm.body), cleanup, failures);
                        }
                    }
                    Op::If {
                        then,
                        elifs,
                        otherwise,
                    } => {
                        rewrite(Arc::make_mut(then), cleanup, failures);
                        for arm in Arc::make_mut(elifs) {
                            rewrite(Arc::make_mut(&mut arm.condition), cleanup, failures);
                            rewrite(Arc::make_mut(&mut arm.body), cleanup, failures);
                        }
                        rewrite(Arc::make_mut(otherwise), cleanup, failures);
                    }
                    _ => {}
                }
            }
        }
        fn contains(ops: &[Op]) -> bool {
            ops.iter().any(|op| match op {
                Op::CheckedUnwrap { .. } => true,
                Op::Loop { condition, body } => contains(condition) || contains(body),
                Op::Match(arms) => arms.iter().any(|arm| contains(&arm.body)),
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    contains(then)
                        || contains(otherwise)
                        || elifs
                            .iter()
                            .any(|arm| contains(&arm.condition) || contains(&arm.body))
                }
                _ => false,
            })
        }
        if !contains(ops) {
            return Ok(());
        }
        let mut cleanup = Vec::new();
        if let Some(scope) = self.checked.last() {
            self.cleanup(scope.start, &mut cleanup);
        }
        let mut failures = Vec::new();
        rewrite(ops, &cleanup.into(), &mut failures);
        if !failures.is_empty() {
            let Some(scope) = self.checked.last_mut() else {
                return Err(at.error("a closure defined in `checked:` cannot leave its block; consume its pipeline inside that block"));
            };
            for error in failures {
                if !scope.failures.contains(&error) {
                    scope.failures.push(error);
                }
            }
        }
        Ok(())
    }

    pub(super) fn lexical_checked(&self) -> bool {
        self.checked.last().is_some_and(|scope| scope.arithmetic)
    }

    /// Unwrap the private failure channel at a call boundary. The callback's
    /// declared Result, if any, is the success payload and remains untouched.
    pub(super) fn checked_call(
        &mut self,
        error: &Ty,
        output: Type,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let Some(scope) = self.checked.last() else {
            return Err(at.error("a closure defined in `checked:` cannot leave its block; define it outside the block instead"));
        };
        let start = scope.start;
        let mut cleanup = Vec::new();
        self.cleanup(start, &mut cleanup);
        let scope = self.checked.last_mut().unwrap();
        for failure in crate::sum::members(error) {
            if !scope.failures.contains(&failure) {
                scope.failures.push(failure);
            }
        }
        ops.push(Op::CheckedUnwrap {
            result: crate::sum::result(output.unwrap_or(Ty::Unit), error.clone()),
            cleanup: cleanup.into(),
        });
        Ok(())
    }

    pub(super) fn checked_named_call(
        &mut self,
        name: &str,
        sig: &FnSig,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let errors = checked::input_errors(&sig.inputs);
        if !errors.is_empty() {
            self.checked_call(
                &crate::sum::anonymous(errors),
                sig.outputs.first().cloned(),
                at,
                ops,
            )?;
        } else if let Some(error) = self.generics.checked_errors.get(name).cloned() {
            self.checked_call(&error, sig.outputs.first().cloned(), at, ops)?;
        }
        Ok(())
    }

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
            arithmetic: true,
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
        if !scope.arithmetic {
            return Ok(false);
        }
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

//! Recoverable integer arithmetic uses each function's declared Result.
//! Explicit `checked:` suites collect local errors; `unchecked:` suites
//! select trapping operations. Nested functions always keep their own
//! contracts. See
//! `book/src/design/language/03-types/numbers.md#checked-blocks`.
use super::*;
use crate::numeric::Checked;

/// The nearest lexical arithmetic mode and its failure boundary.
pub(super) struct Scope {
    /// The first local the block declares; a failure drops it and every
    /// later local.
    pub(super) start: usize,
    /// Failures return the function's declared public Result.
    pub(super) function: bool,
    /// False selects trapping operators without searching outer scopes.
    pub(super) arithmetic: bool,
    /// The errors the block's arithmetic can produce, in source order.
    pub(super) failures: Vec<Ty>,
    /// The block itself contains a trapping integer cast, which it does not
    /// recover; its diagnostic then points at `T.try_from`.
    pub(super) casts: bool,
}

impl Lower<'_> {
    pub(super) fn checked_value(
        &mut self,
        at: &Token,
        suite: Suite,
        body: &[Stmt],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if suite != Suite::Checked {
            return match self.unchecked_suite(at, body, &Tail::Value(expected), ops)? {
                BlockResult::Continues(ty) => Ok(ty),
                BlockResult::Exits => Err(at.error("every path through this `unchecked:` block exits, so it has no value; write it as a statement")),
            };
        }
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
            function: false,
            arithmetic: true,
            failures: Vec::new(),
            casts: false,
        });
        let mut inner = Vec::new();
        let outer = std::mem::replace(&mut self.checked_value_tail, true);
        let branch = std::mem::replace(&mut self.branch_value_tail, false);
        let result = self.block(body, &mut inner, &Tail::Value(success));
        self.checked_value_tail = outer;
        self.branch_value_tail = branch;
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
            let hint = if scope.casts {
                "; casts trap rather than return an error, so use `T.try_from(value)` for a recoverable conversion"
            } else {
                ""
            };
            return Err(at.error(format!(
                "this `checked:` block contains no arithmetic that can fail; remove `checked:`, or move the arithmetic it should check into it{hint}",
            )));
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

    pub(super) fn unchecked_suite(
        &mut self,
        at: &Token,
        body: &[Stmt],
        tail: &Tail,
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        let saved = self.names.clone();
        self.checked.push(Scope {
            start: self.locals.len(),
            function: false,
            arithmetic: false,
            failures: Vec::new(),
            casts: false,
        });
        let outer = std::mem::replace(&mut self.checked_value_tail, false);
        let branch = std::mem::replace(&mut self.branch_value_tail, false);
        let result = self.block(body, ops, tail);
        self.checked_value_tail = outer;
        self.branch_value_tail = branch;
        self.checked.pop().expect("unchecked scope");
        self.names = saved;
        if let Ok(BlockResult::Continues(Some(ty))) = &result
            && ty.contains_reference()
        {
            return Err(at.error("an `unchecked:` block cannot produce a reference or a closure with borrowed captures; produce an owned value"));
        }
        result
    }

    /// The errors `operation` on `ty` can produce.
    fn failures(&self, operation: Checked, ty: &Ty, at: &Token) -> Result<Vec<Ty>> {
        let mut failures = Vec::new();
        if operation.divides() {
            failures.push(self.arithmetic_error("ZeroDivisionError", None, at)?);
        }
        if operation.overflows(ty) {
            failures.push(self.arithmetic_error("OverflowError", Some(ty), at)?);
        }
        if operation.shifts() {
            failures.push(self.arithmetic_error("ShiftCountError", Some(ty), at)?);
        }
        Ok(failures)
    }

    /// Record, in an enclosing `checked:` block, the errors the integer
    /// operator `op` has when no interval proves anything about it. `right`
    /// is its right operand; a literal divisor or count and a masked count
    /// are proofs by themselves.
    ///
    /// A block's error type therefore does not depend on interval proofs:
    /// the same block is accepted, with the same type, for every
    /// instantiation of a generic function, and stays valid when a proof
    /// removes a check. The proved operation is still emitted without its
    /// check. A function's own contract has no such rule: an unused declared
    /// error is already legal there.
    /// See `book/src/design/language/03-types/intervals.md#checked-blocks`.
    pub(super) fn block_errors(
        &mut self,
        op: &str,
        ty: &Ty,
        right: Option<&Expr>,
        at: &Token,
    ) -> Result<()> {
        if !ty.is_int()
            || !self
                .checked
                .last()
                .is_some_and(|scope| scope.arithmetic && !scope.function)
        {
            return Ok(());
        }
        let operation = match op {
            "<<" | ">>" => {
                let (min, end) = ty.int_range().expect("an integer type");
                let width = i128::from((end - min).trailing_zeros());
                if right.is_some_and(|count| {
                    self.known_integer(count).is_some() || self.masked_count(count, width)
                }) {
                    return Ok(());
                }
                if op == "<<" {
                    Checked::ShiftLeft
                } else {
                    Checked::ShiftRight
                }
            }
            _ => match Checked::named(op, self.known_divisor(op, right)) {
                Some(operation) => operation,
                None => return Ok(()),
            },
        };
        let failures = self.failures(operation, ty, at)?;
        let scope = self.checked.last_mut().expect("checked above");
        for failure in failures {
            if !scope.failures.contains(&failure) {
                scope.failures.push(failure);
            }
        }
        Ok(())
    }

    /// Select recoverable arithmetic at the nearest boundary and record its
    /// possible errors. False selects the ordinary operator in unchecked code
    /// or when divisor proofs remove every failure.
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
        let failures = self.failures(operation, ty, at)?;
        if failures.is_empty() {
            return Ok(false);
        }
        if scope.function {
            if self.yield_type.is_some() {
                return Err(at.error("a generator cannot propagate arithmetic errors; handle a local `checked:` result or use an explicit `unchecked:` suite"));
            }
            let declared = self.return_type.clone().flatten();
            let error = match &declared {
                Some(Ty::Enum(result)) if result.is_result() => {
                    Some(&result.get().variants[1].fields[0])
                }
                _ => None,
            };
            for failure in &failures {
                if !error.is_some_and(|error| crate::sum::accepts(error, failure)) {
                    return Err(at.error(format!("arithmetic can fail with {failure}, which is not included in this function's return type {}; declare Result[_, {failure}], handle a local `checked:` result, or deliberately use `unchecked:`", declared.as_ref().map(ToString::to_string).unwrap_or_else(|| "()".into()))));
                }
            }
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

    /// Emit `+`, `-` or `*` on `ty` without a check when `outcome` proves it
    /// cannot overflow. The same verdict keeps `OverflowError` out of the
    /// function's errors and selects the unchecked instruction, so the
    /// checker and native lowering cannot disagree. False emits nothing.
    pub(super) fn proved_arithmetic(
        &self,
        op: &str,
        ty: &Ty,
        outcome: Option<crate::interval::Outcome>,
        ops: &mut Vec<Op>,
    ) -> bool {
        let operation = match op {
            "+" => Op::Add,
            "-" => Op::Sub,
            "*" => Op::Mul,
            _ => return false,
        };
        let Some(outcome) = outcome.filter(|outcome| ty.is_int() && outcome.proved()) else {
            return false;
        };
        ops.push(Op::Proved(Box::new(operation), outcome.value));
        true
    }

    /// `ZeroDivisionError`, or `OverflowError[T]` or `ShiftCountError[T]`
    /// for `operand` `T`.
    fn arithmetic_error(&self, name: &str, operand: Option<&Ty>, at: &Token) -> Result<Ty> {
        let args = operand
            .map(|ty| TypeRef {
                concrete: Some(Box::new(ty.clone())),
                metadata: None,
                at: at.clone(),
                name: None,
                args: Vec::new(),
            })
            .into_iter()
            .collect();
        TypeRef {
            concrete: None,
            metadata: None,
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

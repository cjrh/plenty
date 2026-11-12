//! Divisor and shift-count proofs: literals, module constants and nominal
//! domain values (which retain their proof after unwrapping).
use super::*;
use crate::numeric::{Checked, Divisor};

/// The value of an integer literal operand, optionally negated and
/// parenthesized.
fn literal_integer(operand: &Expr) -> Option<i128> {
    let (negative, inner) = match &ungroup(operand).kind {
        Expression::Unary(sign, inner) if sign == "-" => (true, ungroup(inner)),
        _ => (false, ungroup(operand)),
    };
    let Expression::Number(text) = &inner.kind else {
        return None;
    };
    if text.contains(['.', 'e', 'E', 'f']) {
        return None;
    }
    let digits = text
        .find(|c: char| c.is_ascii_alphabetic())
        .map_or(text.as_str(), |split| &text[..split])
        .replace('_', "");
    let magnitude = digits.parse::<i128>().ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

impl Lower<'_> {
    /// The value of an operand that is an integer literal or a read of an
    /// integer module constant, optionally parenthesized. `consts::fold` has
    /// already replaced each constant's initializer with the literal it
    /// denotes, so a constant proves exactly what that literal proves (issue
    /// #863). Locals and arithmetic, including a negated constant, prove
    /// nothing.
    pub(super) fn known_integer(&self, operand: &Expr) -> Option<i128> {
        match &ungroup(operand).kind {
            Expression::Call(name, args) if args.is_empty() => {
                let getter = self.generics.constants.get(name)?;
                match getter.body.as_slice() {
                    [
                        Stmt {
                            kind: Statement::Expr(value),
                            ..
                        },
                    ] => literal_integer(value),
                    _ => None,
                }
            }
            _ => literal_integer(operand),
        }
    }

    /// The proof a literal or module-constant divisor of `//` or `%`
    /// supplies without intervals: `x // 2` cannot divide by zero, and only
    /// `-1` can overflow (issue #796).
    pub(super) fn known_divisor(&self, op: &str, divisor: Option<&Expr>) -> Divisor {
        if !matches!(op, "//" | "%") {
            return Divisor::Any;
        }
        divisor
            .and_then(|divisor| self.known_integer(divisor))
            .map_or(Divisor::Any, Divisor::literal)
    }

    /// Whether a shift count has the form `n & mask` (either operand order)
    /// with a known mask in `0..width`: the masking idiom, valid without
    /// intervals.
    pub(super) fn masked_count(&self, count: &Expr, width: i128) -> bool {
        let Expression::Binary(op, left, right) = &ungroup(count).kind else {
            return false;
        };
        op == "&"
            && [left, right].into_iter().any(|side| {
                self.known_integer(side)
                    .is_some_and(|mask| (0..width).contains(&mask))
            })
    }

    fn divisor_type(&self, op: &str, value: &Expr) -> Option<(Ty, Divisor)> {
        if !matches!(op, "//" | "%") {
            return None;
        }
        let ty = self.operator_hint(value)?;
        let ty = match &ty {
            Ty::Ref(inner, _) => inner,
            _ => &ty,
        };
        ty.nonzero_element()
            .map(|element| (element, Divisor::NonZero))
            .or_else(|| {
                ty.positive_element()
                    .map(|element| (element, Divisor::Positive))
            })
    }

    pub(super) fn domain_division(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        let Some((element, proof)) = self.divisor_type(op, right) else {
            return Ok(None);
        };
        if self.operator_hint(left).is_some_and(|ty| {
            let ty = match &ty {
                Ty::Ref(inner, _) => inner,
                _ => &ty,
            };
            !ty.is_int()
        }) {
            return Ok(None);
        }
        let (actual, loans) = self.observe_expected(left, Some(element.clone()), ops)?;
        self.same(Some(actual), Some(element.clone()), &left.at)?;
        self.divisor_operand(right, &element, proof, ops)?;
        self.mark_arithmetic(op, &element, at, ops);
        self.division_operation(op, &element, proof, at, ops)?;
        Self::end_reads(loans, ops);
        Ok(Some(Some(element)))
    }

    fn divisor_operand(
        &mut self,
        value: &Expr,
        element: &Ty,
        proof: Divisor,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let (ty, loans) = self.observe(value, ops)?;
        let actual = match proof {
            Divisor::NonZero => ty.nonzero_element(),
            Divisor::Positive => ty.positive_element(),
            Divisor::Any | Divisor::BelowMinusOne | Divisor::Fits | Divisor::NonZeroFits => {
                unreachable!("domain divisors carry a type proof")
            }
        };
        self.same(actual, Some(element.clone()), &value.at)?;
        let Ty::Class(class) = ty else {
            unreachable!("validated integer domain class")
        };
        // A builtin domain is a copyable newtype. Project its integer directly
        // rather than calling `.get()` and introducing caller state/temporaries.
        ops.push(Op::Class(crate::record::ClassOp::Field(class, 0)));
        Self::end_reads(loans, ops);
        Ok(())
    }

    fn division_operation(
        &mut self,
        op: &str,
        ty: &Ty,
        proof: Divisor,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let operation = if op == "//" {
            Checked::FloorDiv(proof)
        } else {
            Checked::Modulo(proof)
        };
        if !self.checked_arith(operation, ty, at, ops)? {
            let operation = if op == "//" {
                Op::FloorDiv(proof)
            } else {
                Op::Modulo(proof)
            };
            ops.push(operation);
        }
        Ok(())
    }

    /// Lower `<<` or `>>` with both operands on the stack. A literal or
    /// module-constant count proves itself valid or is a compile-time error;
    /// any other count may fail with `ShiftCountError[T]` at the nearest
    /// arithmetic boundary.
    /// See `book/src/design/language/03-types/numbers.md#checked-blocks`.
    pub(super) fn shift_operation(
        &mut self,
        kind: IntegerOp,
        ty: &Ty,
        count: &Expr,
        outcome: Option<crate::interval::Outcome>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let (min, end) = ty.int_range().expect("shifts require integers");
        let width = i128::from((end - min).trailing_zeros());
        if let Some(value) = self.known_integer(count) {
            if !(0..width).contains(&value) {
                return Err(count.at.error(format!(
                    "shift count {value} is outside 0..{width} for {ty}"
                )));
            }
        } else if let Some(outcome) = outcome.filter(|outcome| !outcome.bad_count) {
            // The count's interval is inside `0..width - 1`.
            ops.push(Op::Proved(Box::new(Op::Integer(kind)), outcome.value));
            return Ok(());
        } else {
            let operation = if kind == IntegerOp::ShiftLeft {
                Checked::ShiftLeft
            } else {
                Checked::ShiftRight
            };
            if self.checked_arith(operation, ty, at, ops)? {
                return Ok(());
            }
        }
        ops.push(Op::Integer(kind));
        Ok(())
    }

    /// The destination has already been loaded by augmented assignment.
    pub(super) fn numeric_rhs(
        &mut self,
        op: &str,
        ty: &Ty,
        value: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        if let Some((element, proof)) = self.divisor_type(op, value) {
            self.same(Some(element.clone()), Some(ty.clone()), &value.at)?;
            self.divisor_operand(value, &element, proof, ops)?;
            self.mark_arithmetic(op, ty, at, ops);
            self.division_operation(op, ty, proof, at, ops)
        } else {
            self.interval = None;
            let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
            self.same(actual, Some(ty.clone()), &value.at)?;
            self.mark_arithmetic(op, ty, at, ops);
            let operands = intervals::Operands {
                node: 0,
                left: None,
                right: self.operand_interval(value, ty),
            };
            self.numeric_operation(op, ty.clone(), value, operands, at, ops)?;
            Ok(())
        }
    }
}

//! Division by a nominal domain value retains its proof after unwrapping.
use super::*;
use crate::numeric::{Checked, Divisor};

/// The proof an integer literal divisor of `//` or `%` supplies: `x // 2`
/// cannot divide by zero, and only `-1` can overflow. Other divisors,
/// including named constants, prove nothing (issue #796).
pub(super) fn literal_divisor(op: &str, divisor: &Expr) -> Divisor {
    if !matches!(op, "//" | "%") {
        return Divisor::Any;
    }
    literal_integer(divisor).map_or(Divisor::Any, Divisor::literal)
}

/// The value of an integer literal operand, optionally negated and
/// parenthesized. Names, constants and arithmetic are not literals.
pub(super) fn literal_integer(operand: &Expr) -> Option<i128> {
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

/// Whether a shift count has the form `n & mask` (either operand order) with
/// a literal mask in `0..width`. The result then lies in `0..=mask`, so the
/// count is valid without a check; this is the usual masking idiom.
fn masked_count(count: &Expr, width: i128) -> bool {
    let Expression::Binary(op, left, right) = &ungroup(count).kind else {
        return false;
    };
    op == "&"
        && [left, right]
            .into_iter()
            .any(|side| literal_integer(side).is_some_and(|mask| (0..width).contains(&mask)))
}

impl Lower<'_> {
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
            Divisor::Any | Divisor::BelowMinusOne => {
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
            self.push_trapping(operation, ty, ops);
        }
        Ok(())
    }

    /// Lower `<<` or `>>` with both operands on the stack. A literal count
    /// proves itself valid or is a compile-time error; any other count may
    /// fail with `ShiftCountError[T]` at the nearest arithmetic boundary.
    /// See `book/src/design/language/03-types/numbers.md#checked-blocks`.
    pub(super) fn shift_operation(
        &mut self,
        kind: IntegerOp,
        ty: &Ty,
        count: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let (min, end) = ty.int_range().expect("shifts require integers");
        let width = i128::from((end - min).trailing_zeros());
        if let Some(value) = literal_integer(count) {
            if !(0..width).contains(&value) {
                return Err(count.at.error(format!(
                    "shift count {value} is outside 0..{width} for {ty}"
                )));
            }
        } else if !masked_count(count, width) {
            let operation = if kind == IntegerOp::ShiftLeft {
                Checked::ShiftLeft
            } else {
                Checked::ShiftRight
            };
            if self.checked_arith(operation, ty, at, ops)? {
                return Ok(());
            }
        }
        self.push_trapping(Op::Integer(kind), ty, ops);
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
            let actual = self.expr_expected(value, Some(ty.clone()), ops)?;
            self.same(actual, Some(ty.clone()), &value.at)?;
            self.mark_arithmetic(op, ty, at, ops);
            self.numeric_operation(op, ty.clone(), value, at, ops)?;
            Ok(())
        }
    }
}

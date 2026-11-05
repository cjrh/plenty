//! Division by a nominal domain value retains its proof after unwrapping.
use super::*;
use crate::numeric::{Checked, Divisor};

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
            Divisor::Any => unreachable!("domain divisors carry a proof"),
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
            self.numeric_operation(op, ty.clone(), at, ops)?;
            Ok(())
        }
    }
}

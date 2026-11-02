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
        self.divisor_operand(right, &element, ops)?;
        self.mark_arithmetic(op, &element, at, ops);
        self.division_operation(op, &element, proof, at, ops)?;
        Self::end_reads(loans, ops);
        Ok(Some(Some(element)))
    }

    fn divisor_operand(&mut self, value: &Expr, element: &Ty, ops: &mut Vec<Op>) -> Result<()> {
        let get = Expr {
            at: value.at.clone(),
            kind: Expression::Method(
                Box::new(value.clone()),
                "get".into(),
                vec![],
                Pos::of(&value.at),
            ),
        };
        let actual = self.expr(&get, ops)?;
        self.same(actual, Some(element.clone()), &value.at)
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
            ops.push(if op == "//" {
                Op::FloorDiv(proof)
            } else {
                Op::Modulo(proof)
            });
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
            self.divisor_operand(value, &element, ops)?;
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

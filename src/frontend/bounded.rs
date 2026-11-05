//! Inclusive integer intervals. Only literal admission and checked conversion
//! reach the private constructor; ordinary arithmetic explicitly extracts T.
use super::*;
use value_generics::Constant;

impl TypeAliases {
    pub(super) fn bounded(&self, ty: &Ty) -> Option<(Ty, Constant, Constant)> {
        let (name, args) = self.data.arguments(ty)?;
        if name != "Bounded" {
            return None;
        }
        let [Argument::Type(t), Argument::Value(lo), Argument::Value(hi)] = args.as_slice() else {
            return None;
        };
        Some((t.clone(), lo.clone(), hi.clone()))
    }
}
impl Lower<'_> {
    fn bounded_construct(&self, ty: &Ty, value: Expr) -> Expr {
        let at = self
            .aliases
            .class_declaration("Bounded")
            .expect("builtin Bounded");
        Expr {
            at: at.clone(),
            kind: Expression::Constructor(Box::new(generics::type_ref(ty, &at)), vec![value]),
        }
    }
    pub(super) fn bounded_literal(&mut self, e: &Expr, ty: &Ty, ops: &mut Vec<Op>) -> Result<Type> {
        let (element, lo, hi) = self.aliases.bounded(ty).unwrap();
        let value = consts::generic_value(e, &element, self.aliases)?;
        if value.value < lo.value || value.value > hi.value {
            return Err(e.at.error(format!(
                "literal {value} is outside `{ty}`'s inclusive bounds {lo}..{hi}"
            )));
        }
        self.expr(&self.bounded_construct(ty, value.expression(&e.at)), ops)
    }
    pub(super) fn bounded_try_from(
        &mut self,
        ty: &Ty,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (element, lo, hi) = self.aliases.bounded(ty).unwrap();
        let [value] = args else {
            return Err(at.error("Bounded.try_from takes one integer"));
        };
        let actual = self.expr_expected(value, Some(element.clone()), ops)?;
        let actual = if let Some(Ty::Ref(inner, _)) = actual {
            ops.push(Op::ReadRef((*inner).clone()));
            Some((*inner).clone())
        } else {
            actual
        };
        self.same(actual, Some(element.clone()), &value.at)?;
        let slot = self.slot(element.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        let held = format!("$bounded{slot}");
        self.bind(
            held.clone(),
            Local {
                slot,
                ty: element,
                mutable: false,
            },
        );
        let expression = |kind| Expr {
            at: at.clone(),
            kind,
        };
        let name = || expression(Expression::Name(held.clone()));
        let lower = expression(Expression::Binary(
            "<".into(),
            Box::new(name()),
            Box::new(lo.expression(at)),
        ));
        let upper = expression(Expression::Binary(
            ">".into(),
            Box::new(name()),
            Box::new(hi.expression(at)),
        ));
        let rejected = expression(Expression::Call(
            "Err".into(),
            vec![expression(Expression::Member(
                Box::new(expression(Expression::Name("RangeError".into()))),
                "OutOfRange".into(),
                Pos::default(),
            ))],
        ));
        let accepted = expression(Expression::Call(
            "Ok".into(),
            vec![self.bounded_construct(ty, name())],
        ));
        let conversion = expression(Expression::Conditional {
            condition: Box::new(expression(Expression::Binary(
                "or".into(),
                Box::new(lower),
                Box::new(upper),
            ))),
            yes: Box::new(rejected),
            no: Box::new(accepted),
        });
        let result = crate::sum::result(ty.clone(), crate::sum::range_error());
        let out = self.expr_expected(&conversion, Some(result.clone()), ops);
        self.names.remove(&held);
        out?;
        Ok(Some(result))
    }
}

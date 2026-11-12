//! Inclusive integer intervals. Only literal admission, checked conversion
//! and conversion of a value in a proved interval reach the private
//! constructor; ordinary arithmetic explicitly extracts T.
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

    /// `Bounded[T, lo, hi].from(value)`: the conversion with no run-time
    /// check, accepted only when the argument's
    /// [interval](crate::interval) lies inside `lo..hi` (issues #937 and
    /// #952). A `Bounded[T, a, b]` argument has the interval `a..b`; an
    /// integer argument has the interval its lowering recorded, so a
    /// class's operator hook proves nothing.
    pub(super) fn bounded_from(
        &mut self,
        ty: &Ty,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (element, lo, hi) = self.aliases.bounded(ty).unwrap();
        let [value] = args else {
            return Err(at.error("Bounded.from takes one value"));
        };
        let bounds = crate::interval::Interval {
            lo: lo.value,
            hi: hi.value,
        };
        let hinted = self.operator_hint(value).is_some_and(|hint| {
            let hint = match &hint {
                Ty::Ref(inner, _) => inner,
                other => other,
            };
            self.aliases.bounded(hint).is_some()
        });
        self.interval = None;
        let source = if hinted {
            self.expr(value, ops)?
        } else {
            self.expr_expected(value, Some(element.clone()), ops)?
        };
        let source = source.ok_or_else(|| value.at.error("expected a value, got ()"))?;
        // Read before the dereference: a reference has no recorded interval.
        let recorded = self.operand_interval(value, &source);
        let source = if let Ty::Ref(inner, _) = source {
            ops.push(Op::ReadRef((*inner).clone()));
            (*inner).clone()
        } else {
            source
        };
        let proved = if let Some((inner, from, to)) = self.aliases.bounded(&source) {
            self.same(Some(inner), Some(element.clone()), &value.at)?;
            crate::interval::Interval {
                lo: from.value,
                hi: to.value,
            }
        } else {
            self.same(Some(source.clone()), Some(element.clone()), &value.at)?;
            recorded
                .or_else(|| crate::interval::Interval::of(&element))
                .expect("a bounded integer stores an integer")
        };
        if !proved.within(bounds) {
            return Err(value.at.error(format!(
                "this value is in {proved}, which is not inside `{ty}`'s inclusive bounds {lo}..{hi}; use `Bounded.try_from(value)?` to check it at run time"
            )));
        }
        let slot = self.slot(source.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        let held = format!("$bounded{slot}");
        let bounded = source != element;
        self.bind(
            held.clone(),
            Local {
                slot,
                ty: source,
                mutable: false,
            },
        );
        let expression = |kind| Expr {
            at: at.clone(),
            kind,
        };
        let name = expression(Expression::Name(held.clone()));
        let integer = if bounded {
            expression(Expression::Method(
                Box::new(name),
                "get".into(),
                Vec::new(),
                Pos::default(),
            ))
        } else {
            name
        };
        let out = self.expr(&self.bounded_construct(ty, integer), ops);
        self.names.remove(&held);
        out
    }
}

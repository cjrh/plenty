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
    /// check, accepted only when one of these rules gives an interval inside
    /// `lo..hi` (issue #937):
    ///
    /// - `x % c`, `c` a literal or constant above zero: `0..c - 1`, because
    ///   `%` is the floored remainder;
    /// - `x & c` or `c & x`, `c` a literal or constant of at least zero:
    ///   `0..c`, because the result has no bit that `c` lacks;
    /// - a `Bounded[T, a, b]` value: `a..b`.
    ///
    /// The operator must be the integer one: the last operation lowered is
    /// checked, so a class's `__mod__` or `__and__` hook proves nothing.
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
        let unproved = || {
            value.at.error(format!(
                "`Bounded.from` takes a value the compiler proves is in {lo}..{hi}: `x % c` or `x & c` with a constant `c`, or a `Bounded` value; use `Bounded.try_from(value)?` to check a value at run time"
            ))
        };
        let form = match &ungroup(value).kind {
            Expression::Binary(op, _, right) if op == "%" => self
                .known_integer(right)
                .filter(|divisor| *divisor > 0)
                .map(|divisor| (IntegerOp::Mod, divisor - 1)),
            Expression::Binary(op, left, right) if op == "&" => [left, right]
                .into_iter()
                .filter_map(|side| self.known_integer(side))
                .filter(|mask| *mask >= 0)
                .min()
                .map(|mask| (IntegerOp::And, mask)),
            _ => None,
        };
        let (source, proved) = if let Some((operator, most)) = form {
            let actual = self.expr_expected(value, Some(element.clone()), ops)?;
            self.same(actual, Some(element.clone()), &value.at)?;
            let last = ops
                .iter()
                .rev()
                .find(|op| !matches!(op, Op::Site(_) | Op::CallerSite(_)));
            let integer = matches!(
                (operator, last),
                (IntegerOp::Mod, Some(Op::Modulo(_)))
                    | (IntegerOp::And, Some(Op::Integer(crate::op::IntegerOp::And)))
            );
            if !integer {
                return Err(unproved());
            }
            (element.clone(), (0, most))
        } else {
            let source = self.expr(value, ops)?.ok_or_else(unproved)?;
            let source = if let Ty::Ref(inner, _) = source {
                ops.push(Op::ReadRef((*inner).clone()));
                (*inner).clone()
            } else {
                source
            };
            let Some((inner, from, to)) = self.aliases.bounded(&source) else {
                return Err(unproved());
            };
            self.same(Some(inner), Some(element.clone()), &value.at)?;
            (source, (from.value, to.value))
        };
        if proved.0 < lo.value || proved.1 > hi.value {
            return Err(value.at.error(format!(
                "this value is in {}..{}, which is not inside `{ty}`'s inclusive bounds {lo}..{hi}; use `Bounded.try_from(value)?` to check it at run time",
                proved.0, proved.1
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

/// The operator of a `Bounded.from` argument whose result has a known range.
#[derive(Clone, Copy)]
enum IntegerOp {
    Mod,
    And,
}

//! `NonZero[T]`, a builtin integer that cannot be zero. It is a private
//! `@newtype @copy` class declared with the arithmetic errors, so its value
//! has the integer's representation. Values come only from a nonzero literal
//! where one is expected or from `NonZero.try_from`. See
//! book/src/design/language/03-types/numbers.md#nonzero-integers.
use super::*;

pub(super) const CLASS: &str = "NonZero";

/// Operators a `NonZero` forwards: those that cannot produce zero from two
/// nonzero operands, plus the comparisons. `+` and `-` forward but give `T`.
pub(super) fn forwards(op: &str) -> bool {
    matches!(op, "*" | "+" | "-" | "<" | "<=" | ">" | ">=")
}

pub(super) fn unsupported(op: &str, ty: &Ty) -> String {
    format!(
        "operator `{op}` does not accept `{ty}`, since its result could be zero; read the integer with `.get()`"
    )
}

/// An unsuffixed integer literal, possibly negated.
pub(super) fn literal(e: &Expr) -> bool {
    let integer = |e: &Expr| matches!(&ungroup(e).kind, Expression::Number(n) if !n.contains(['.', 'e', 'E']));
    match &ungroup(e).kind {
        Expression::Unary(sign, inner) if sign == "-" => integer(inner),
        _ => integer(e),
    }
}

fn zero(value: &Value) -> bool {
    match *value {
        Value::I8(v) => v == 0,
        Value::I16(v) => v == 0,
        Value::I32(v) => v == 0,
        Value::I64(v) => v == 0,
        Value::U8(v) => v == 0,
        Value::U16(v) => v == 0,
        Value::U32(v) => v == 0,
        Value::U64(v) => v == 0,
    }
}

impl Lower<'_> {
    /// `NonZero[element]`.
    pub(super) fn nonzero_type(&self, element: &Ty, at: &Token) -> Result<Ty> {
        TypeRef {
            concrete: None,
            parameter: false,
            at: at.clone(),
            name: Some(CLASS.into()),
            args: vec![TypeRef {
                concrete: Some(Box::new(element.clone())),
                parameter: false,
                at: at.clone(),
                name: None,
                args: Vec::new(),
            }],
        }
        .resolve(self.aliases)?
        .ok_or_else(|| at.error("`NonZero` is missing"))
    }

    /// `NonZero(value)`, written at the class's declaration so that its
    /// private constructor is reachable. Callers have ruled out zero.
    fn nonzero_construct(&self, ty: &Ty, value: Expr) -> Result<Expr> {
        let Ty::Class(class) = ty else {
            unreachable!("NonZero is a class")
        };
        let at = self
            .aliases
            .newtype_annotation(&class.name)
            .ok_or_else(|| value.at.error("`NonZero` is missing"))?;
        Ok(Expr {
            at: at.clone(),
            kind: Expression::Constructor(
                Box::new(TypeRef {
                    concrete: Some(Box::new(ty.clone())),
                    parameter: false,
                    at,
                    name: None,
                    args: vec![],
                }),
                vec![value],
            ),
        })
    }

    /// An integer literal, possibly negated, where `NonZero[T]` is expected:
    /// checked and converted at compile time.
    pub(super) fn nonzero_literal(&mut self, e: &Expr, ty: &Ty, ops: &mut Vec<Op>) -> Result<Type> {
        let element = ty.nonzero_element().unwrap();
        let mut literal = Vec::new();
        self.expr_expected(e, Some(element), &mut literal)?;
        if let [Op::PushInt(value)] = literal.as_slice()
            && zero(value)
        {
            return Err(e.at.error(format!(
                "`0` is not a `{ty}`: a NonZero value cannot be zero"
            )));
        }
        let construct = self.nonzero_construct(ty, e.clone())?;
        self.expr(&construct, ops)
    }

    /// `NonZero.try_from(value)` or `NonZero[T].try_from(value)`:
    /// `Result[NonZero[T], RangeError]`, `Err(RangeError.OutOfRange)` for zero.
    /// The bare form takes `T` from the value.
    pub(super) fn nonzero_try_from(
        &mut self,
        element: Option<Ty>,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let [value] = args else {
            return Err(at.error("NonZero.try_from takes one integer"));
        };
        let actual = self.expr_expected(value, element.clone(), ops)?;
        let actual = match actual {
            Some(Ty::Ref(inner, _)) if inner.is_int() => {
                ops.push(Op::ReadRef((*inner).clone()));
                (*inner).clone()
            }
            Some(ty) if ty.is_int() => ty,
            other => {
                return Err(value.at.error(format!(
                    "NonZero.try_from takes an integer, not {}",
                    type_name(other)
                )));
            }
        };
        if let Some(element) = &element
            && *element != actual
        {
            return Err(value.at.error(format!(
                "`NonZero[{element}].try_from` takes `{element}`, not `{actual}`; convert it first with `{element}.try_from(value)?`"
            )));
        }
        let ty = self.nonzero_type(&actual, at)?;
        let slot = self.slot(actual.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        let held = format!("$nonzero{slot}");
        self.bind(
            held.clone(),
            Local {
                slot,
                ty: actual,
                mutable: false,
            },
        );
        let expression = |kind| Expr {
            at: at.clone(),
            kind,
        };
        let name = || expression(Expression::Name(held.clone()));
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
            vec![self.nonzero_construct(&ty, name())?],
        ));
        let conversion = expression(Expression::Conditional {
            condition: Box::new(expression(Expression::Binary(
                "==".into(),
                Box::new(name()),
                Box::new(expression(Expression::Number("0".into()))),
            ))),
            yes: Box::new(rejected),
            no: Box::new(accepted),
        });
        let result = crate::sum::result(ty, crate::sum::range_error());
        let out = self.expr_expected(&conversion, Some(result.clone()), ops);
        self.names.remove(&held);
        out?;
        Ok(Some(result))
    }

    /// `value OP literal` or `literal OP value` for a `NonZero[T]` value: the
    /// literal is a `NonZero[T]` too, so `step * 2` and `step == 1` keep
    /// their nominal operands. A literal has no effects, so it is lowered
    /// first and held.
    pub(super) fn nonzero_literal_operand(
        &mut self,
        op: &str,
        left: &Expr,
        right: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Option<Type>> {
        if !(forwards(op) || matches!(op, "==" | "!=")) {
            return Ok(None);
        }
        let nonzero = |hint: Option<Ty>| {
            hint.map(|ty| match ty {
                Ty::Ref(inner, _) => (*inner).clone(),
                other => other,
            })
            .filter(|ty| ty.nonzero_element().is_some())
        };
        let (ty, constant, left_literal) = if literal(right)
            && let Some(ty) = nonzero(self.operator_hint(left))
        {
            (ty, right, false)
        } else if literal(left)
            && let Some(ty) = nonzero(self.operator_hint(right))
        {
            (ty, left, true)
        } else {
            return Ok(None);
        };
        self.nonzero_literal(constant, &ty, ops)?;
        let slot = self.slot(ty.clone(), &constant.at)?;
        ops.push(Op::StoreLocal(slot));
        let held = format!("$nonzero{slot}");
        self.bind(
            held.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        let name = Expr {
            at: constant.at.clone(),
            kind: Expression::Name(held.clone()),
        };
        let (a, b) = if left_literal {
            (name, right.clone())
        } else {
            (left.clone(), name)
        };
        let expression = Expr {
            at: at.clone(),
            kind: Expression::Binary(op.into(), Box::new(a), Box::new(b)),
        };
        let result = self.expr(&expression, ops);
        self.names.remove(&held);
        result.map(Some)
    }
}

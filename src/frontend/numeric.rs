//! Numeric built-ins: the functions `abs`, `min`, `max`, and `sum`, the
//! methods on a numeric value, and the constants written `T.NAME`. Every
//! two-operand form takes operands of one type, as the arithmetic operators
//! do. See `book/src/design/facilities/01-builtins/numbers.md`.
use super::*;
use crate::numeric::{FloatOp, NumericOp};

pub(super) fn function(name: &str) -> bool {
    matches!(name, "abs" | "min" | "max" | "sum")
}

pub(super) fn method(name: &str) -> bool {
    name == "pow" || float_method(name).is_some()
}

fn float_method(name: &str) -> Option<NumericOp> {
    Some(match name {
        "sqrt" => NumericOp::Sqrt,
        "exp" => NumericOp::Float(FloatOp::Exp),
        "log" => NumericOp::Float(FloatOp::Log),
        "sin" => NumericOp::Float(FloatOp::Sin),
        "cos" => NumericOp::Float(FloatOp::Cos),
        "floor" => NumericOp::Float(FloatOp::Floor),
        "ceil" => NumericOp::Float(FloatOp::Ceil),
        "fmod" => NumericOp::Float(FloatOp::Fmod),
        _ => return None,
    })
}

fn constant(ty: &Ty, name: &str) -> Option<Op> {
    if let (Some((min, end)), "MIN" | "MAX") = (ty.int_range(), name) {
        let value = if name == "MIN" { min } else { end - 1 };
        return Some(Op::PushInt(match ty {
            Ty::I8 => Value::I8(value as i8),
            Ty::I16 => Value::I16(value as i16),
            Ty::I32 => Value::I32(value as i32),
            Ty::I64 => Value::I64(value as i64),
            Ty::U8 => Value::U8(value as u8),
            Ty::U16 => Value::U16(value as u16),
            Ty::U32 => Value::U32(value as u32),
            _ => Value::U64(value as u64),
        }));
    }
    let bits = match (ty, name) {
        (Ty::F32, "PI") => u64::from(std::f32::consts::PI.to_bits()),
        (Ty::F32, "E") => u64::from(std::f32::consts::E.to_bits()),
        (Ty::F64, "PI") => std::f64::consts::PI.to_bits(),
        (Ty::F64, "E") => std::f64::consts::E.to_bits(),
        _ => return None,
    };
    Some(Op::PushFloat {
        bits,
        ty: ty.clone(),
    })
}

impl Lower<'_> {
    /// The constant `T.NAME` of a numeric type.
    pub(super) fn numeric_constant(
        &self,
        ty: Ty,
        name: &str,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        ops.push(constant(&ty, name).ok_or_else(|| {
            at.error(format!(
                "unknown constant `{ty}.{name}`; integer types provide `MIN` and `MAX`, float types provide `PI` and `E`"
            ))
        })?);
        Ok(ty)
    }

    /// The type a numeric built-in produces, read from its operands without
    /// lowering them, so that it can type an unsuffixed literal beside it.
    pub(super) fn numeric_builtin_hint(&self, e: &Expr) -> Type {
        match &ungroup(e).kind {
            Expression::Member(base, name) => self
                .qualified_type(base)
                .ok()
                .flatten()
                .filter(|ty| constant(ty, name).is_some()),
            Expression::Call(name, args) if function(name) && !self.names.contains_key(name) => {
                if name == "sum" {
                    match self.place_type(args.first()?)? {
                        Ty::List(element) => Some((*element).clone()).filter(Ty::is_numeric),
                        _ => None,
                    }
                } else {
                    args.iter().find_map(|argument| self.numeric_hint(argument))
                }
            }
            Expression::Method(base, name, args) if method(name) => self
                .numeric_hint(base)
                .or_else(|| args.iter().find_map(|argument| self.numeric_hint(argument))),
            _ => None,
        }
    }

    /// `abs(x)`, `min(a, b)`, `max(a, b)`, and `sum(xs)`. `expected` types
    /// unsuffixed literals when no operand has a type of its own.
    pub(super) fn numeric_call(
        &mut self,
        name: &str,
        args: &[Expr],
        expected: Type,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if name == "sum" {
            return self.list_sum(args, at, ops);
        }
        let hint = args
            .iter()
            .find_map(|argument| self.numeric_hint(argument))
            .or(expected.filter(Ty::is_numeric));
        if name == "abs" {
            let [value] = args else {
                return Err(at.error("abs takes one number"));
            };
            let (ty, loans) = self.observe_expected(value, hint, ops)?;
            if !ty.is_numeric() {
                return Err(value.at.error(format!(
                    "abs requires a number, got {}",
                    type_name(Some(ty))
                )));
            }
            if ty.is_int() {
                self.mark(at, ops);
            }
            ops.push(Op::Numeric(NumericOp::Abs));
            Self::end_reads(loans, ops);
            return Ok(ty);
        }
        let [left, right] = args else {
            return Err(at.error(format!(
                "{name} takes two numbers of one type; nest calls for more, and use a loop over a collection"
            )));
        };
        let (ty, mut loans) = self.observe_expected(left, hint, ops)?;
        if !ty.is_numeric() {
            return Err(left.at.error(format!(
                "{name} requires numbers, got {}",
                type_name(Some(ty))
            )));
        }
        loans.extend(self.same_numeric_operand(left, &ty, right, ops)?);
        ops.push(Op::Numeric(if name == "min" {
            NumericOp::Min
        } else {
            NumericOp::Max
        }));
        Self::end_reads(loans, ops);
        Ok(ty)
    }

    /// Lower `right`, which must have the numeric type `ty` that `left` has.
    fn same_numeric_operand(
        &mut self,
        left: &Expr,
        ty: &Ty,
        right: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Vec<usize>> {
        let (other, loans) = self.observe_expected(right, Some(ty.clone()), ops)?;
        if other.is_numeric() && other != *ty {
            return Err(mixed_numeric(left, ty, right, &other));
        }
        self.same(Some(other), Some(ty.clone()), &right.at)?;
        Ok(loans)
    }

    /// `sum(xs)` observes its list: a binding or field is borrowed for the
    /// call, as `len` borrows it.
    fn list_sum(&mut self, args: &[Expr], at: &Token, ops: &mut Vec<Op>) -> Result<Ty> {
        let [list] = args else {
            return Err(at.error("sum takes one list of numbers"));
        };
        let numbers = |ty: &Ty| match ty {
            Ty::List(element) if element.is_numeric() => Ok((**element).clone()),
            other => Err(list.at.error(format!(
                "sum requires a list of numbers, got {}; add other values in a loop",
                type_name(Some(other.clone()))
            ))),
        };
        let (element, borrowed, loans) = if matches!(self.place_type(list), Some(Ty::List(_))) {
            let element = numbers(&self.place_type(list).unwrap())?;
            let (_, loan) = self.borrow(list, false, ops)?;
            (element, true, vec![loan])
        } else {
            let (ty, loans) = self.observe(list, ops)?;
            (numbers(&ty)?, false, loans)
        };
        if element.is_int() {
            self.mark(at, ops);
        }
        ops.push(Op::Numeric(NumericOp::ListSum {
            element: element.clone(),
            borrowed,
        }));
        Self::end_reads(loans, ops);
        Ok(element)
    }

    /// A method named by `method` on the numeric `receiver`, already lowered.
    pub(super) fn numeric_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        receiver: (Ty, Vec<usize>),
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let (ty, mut loans) = receiver;
        let operation = float_method(name).unwrap_or(NumericOp::Pow);
        if operation != NumericOp::Pow && !ty.is_float() {
            return Err(base.at.error(format!(
                "{name} requires a float, got {ty}; convert with `f64(...)`"
            )));
        }
        if matches!(operation, NumericOp::Pow | NumericOp::Float(FloatOp::Fmod)) {
            let [other] = args else {
                return Err(base
                    .at
                    .error(format!("{name} takes one argument of the receiver's type")));
            };
            loans.extend(self.same_numeric_operand(base, &ty, other, ops)?);
        } else if !args.is_empty() {
            return Err(base.at.error(format!("{name} takes no arguments")));
        }
        if ty.is_int() {
            self.mark(&base.at, ops);
        }
        ops.push(Op::Numeric(operation));
        Self::end_reads(loans, ops);
        Ok(ty)
    }
}

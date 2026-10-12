//! Numeric built-ins: the methods on a numeric value and the constants
//! written `T.NAME`. A two-operand method takes operands of one type, as the
//! arithmetic operators do. See
//! `book/src/design/facilities/01-builtins/numbers.md`.
use super::*;
use crate::numeric::{FloatOp, NumericOp};

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
        "tan" => NumericOp::Float(FloatOp::Tan),
        "asin" => NumericOp::Float(FloatOp::Asin),
        "acos" => NumericOp::Float(FloatOp::Acos),
        "atan" => NumericOp::Float(FloatOp::Atan),
        "atan2" => NumericOp::Float(FloatOp::Atan2),
        "sinh" => NumericOp::Float(FloatOp::Sinh),
        "cosh" => NumericOp::Float(FloatOp::Cosh),
        "tanh" => NumericOp::Float(FloatOp::Tanh),
        "asinh" => NumericOp::Float(FloatOp::Asinh),
        "acosh" => NumericOp::Float(FloatOp::Acosh),
        "atanh" => NumericOp::Float(FloatOp::Atanh),
        "exp2" => NumericOp::Float(FloatOp::Exp2),
        "expm1" => NumericOp::Float(FloatOp::Expm1),
        "log1p" => NumericOp::Float(FloatOp::Log1p),
        "log2" => NumericOp::Float(FloatOp::Log2),
        "log10" => NumericOp::Float(FloatOp::Log10),
        "cbrt" => NumericOp::Float(FloatOp::Cbrt),
        "trunc" => NumericOp::Float(FloatOp::Trunc),
        "fabs" => NumericOp::Float(FloatOp::Abs),
        "copysign" => NumericOp::Float(FloatOp::Copysign),
        "round" => NumericOp::Float(FloatOp::Round),
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
        (Ty::F32, "TAU") => u64::from(std::f32::consts::TAU.to_bits()),
        (Ty::F64, "TAU") => std::f64::consts::TAU.to_bits(),
        (Ty::F32, "INF") => u64::from(f32::INFINITY.to_bits()),
        (Ty::F64, "INF") => f64::INFINITY.to_bits(),
        (Ty::F32, "NAN") => u64::from(f32::NAN.to_bits()),
        (Ty::F64, "NAN") => f64::NAN.to_bits(),
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
                "unknown constant `{ty}.{name}`; integer types provide `MIN` and `MAX`, float types provide `PI`, `E`, `TAU`, `INF`, and `NAN`"
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
            Expression::Method(base, name, args) if method(name) => self
                .numeric_hint(base)
                .or_else(|| args.iter().find_map(|argument| self.numeric_hint(argument))),
            _ => None,
        }
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
        if operation == NumericOp::Pow
            || matches!(operation, NumericOp::Float(op) if op.is_binary())
        {
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

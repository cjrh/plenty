//! Typed numeric built-in operations shared by checking and native lowering.
//! The rules are in `book/src/design/facilities/01-builtins/numbers.md`.
use crate::op::Ty;

/// A float function evaluated by the runtime. The discriminants are the
/// `kind` argument of `plenty_f64_math` and `plenty_f32_math`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatOp {
    Exp = 0,
    Log = 1,
    Sin = 2,
    Cos = 3,
    Floor = 4,
    Ceil = 5,
    Pow = 6,
    Fmod = 7,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumericOp {
    /// `T T -> T`. An integer result outside `T` and a negative integer
    /// exponent trap; floats follow the platform `pow`.
    Pow,
    /// `F -> F`.
    Sqrt,
    /// `F -> F`, or `F F -> F` for `Pow` and `Fmod`.
    Float(FloatOp),
}

impl NumericOp {
    /// Number of same-typed operands.
    fn operands(&self) -> usize {
        match self {
            Self::Pow | Self::Float(FloatOp::Pow | FloatOp::Fmod) => 2,
            Self::Sqrt | Self::Float(_) => 1,
        }
    }

    pub fn check(&self, stack: &mut Vec<Ty>) -> Result<(), String> {
        let ty = stack.pop().ok_or("numeric operation stack underflow")?;
        let accepted = match self {
            Self::Sqrt | Self::Float(_) => ty.is_float(),
            Self::Pow => ty.is_numeric(),
        };
        if !accepted {
            return Err(format!("numeric operation does not accept {ty}"));
        }
        if self.operands() == 2 && stack.pop().as_ref() != Some(&ty) {
            return Err("numeric operations require same-typed operands".into());
        }
        stack.push(ty);
        Ok(())
    }
}

/// The `kind` argument of `plenty_int_pow`: the type's width in bits, plus
/// one for a signed type.
pub fn int_kind(ty: &Ty) -> i64 {
    match ty {
        Ty::I8 => 9,
        Ty::I16 => 17,
        Ty::I32 => 33,
        Ty::I64 => 65,
        Ty::U8 => 8,
        Ty::U16 => 16,
        Ty::U32 => 32,
        Ty::U64 => 64,
        _ => panic!("non-integer in int_kind"),
    }
}

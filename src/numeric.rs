//! Typed numeric built-in operations shared by checking and native lowering.
//! The rules are in `book/src/design/facilities/01-builtins/numbers.md`.
use crate::op::Ty;

/// A float function. The discriminants are the `kind` argument of
/// `plenty_f64_math` and `plenty_f32_math`, which evaluate those that native
/// lowering does not expand inline (`fabs`, `copysign` and rounding are inline).
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
    Tan = 8,
    Asin = 9,
    Acos = 10,
    Atan = 11,
    Atan2 = 12,
    Sinh = 13,
    Cosh = 14,
    Tanh = 15,
    Asinh = 16,
    Acosh = 17,
    Atanh = 18,
    Exp2 = 19,
    Expm1 = 20,
    Log1p = 21,
    Log2 = 22,
    Log10 = 23,
    Cbrt = 24,
    Trunc = 25,
    Abs = 26,
    Copysign = 27,
    Round = 28,
}

impl FloatOp {
    pub fn is_binary(self) -> bool {
        matches!(self, Self::Pow | Self::Fmod | Self::Atan2 | Self::Copysign)
    }
}

/// Same-width IEEE representation conversions used only by `std.encoding`.
/// These move bits between register classes without floating-point arithmetic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatBits {
    F32ToU32,
    U32ToF32,
    F64ToU64,
    U64ToF64,
}

impl FloatBits {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "__encoding_f32_bits" => Self::F32ToU32,
            "__encoding_f32_from_bits" => Self::U32ToF32,
            "__encoding_f64_bits" => Self::F64ToU64,
            "__encoding_f64_from_bits" => Self::U64ToF64,
            _ => return None,
        })
    }

    pub fn signature(self) -> (Ty, Ty) {
        match self {
            Self::F32ToU32 => (Ty::F32, Ty::U32),
            Self::U32ToF32 => (Ty::U32, Ty::F32),
            Self::F64ToU64 => (Ty::F64, Ty::U64),
            Self::U64ToF64 => (Ty::U64, Ty::F64),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumericOp {
    /// `T T -> T`. An integer result outside `T` and a negative integer
    /// exponent trap; floats follow the platform `pow`.
    Pow,
    /// `F -> F`.
    Sqrt,
    /// `F -> F`, or `F F -> F` for the binary `FloatOp`s (see `FloatOp::is_binary`).
    Float(FloatOp),
    /// One scalar register, reinterpreted without changing its bits.
    Bits(FloatBits),
}

impl NumericOp {
    /// Number of same-typed operands.
    fn operands(&self) -> usize {
        match self {
            Self::Pow => 2,
            Self::Float(op) if op.is_binary() => 2,
            Self::Sqrt | Self::Float(_) | Self::Bits(_) => 1,
        }
    }

    pub fn check(&self, stack: &mut Vec<Ty>) -> Result<(), String> {
        let ty = stack.pop().ok_or("numeric operation stack underflow")?;
        let accepted = match self {
            Self::Sqrt | Self::Float(_) => ty.is_float(),
            Self::Pow => ty.is_numeric(),
            Self::Bits(kind) => ty == kind.signature().0,
        };
        if !accepted {
            return Err(format!("numeric operation does not accept {ty}"));
        }
        if self.operands() == 2 && stack.pop().as_ref() != Some(&ty) {
            return Err("numeric operations require same-typed operands".into());
        }
        stack.push(match self {
            Self::Bits(kind) => kind.signature().1,
            _ => ty,
        });
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

/// Proof supplied by the divisor's nominal type or by an integer literal
/// divisor, retained until native lowering. The literal rule is in
/// `book/src/design/language/03-types/numbers.md#checked-blocks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Divisor {
    Any,
    /// Never zero; may be `-1`.
    NonZero,
    /// Greater than zero.
    Positive,
    /// At most `-2`: a negative literal other than `-1`.
    BelowMinusOne,
}

impl Divisor {
    /// The proof an integer literal divisor with `value` supplies.
    pub fn literal(value: i128) -> Self {
        match value {
            0 => Self::Any,
            -1 => Self::NonZero,
            v if v > 0 => Self::Positive,
            _ => Self::BelowMinusOne,
        }
    }

    pub fn may_be_minus_one(self) -> bool {
        matches!(self, Self::Any | Self::NonZero)
    }
}

/// Arithmetic that exits the innermost enclosing `checked:` block with an
/// error instead of trapping: `+ - *`, unary `-` (as `0 - x`), `//`, `%` and
/// `pow` on integers, and float division for `div_checked`. The rules are in
/// `book/src/design/language/03-types/numbers.md#checked-blocks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Checked {
    Add,
    Sub,
    Mul,
    FloorDiv(Divisor),
    Modulo(Divisor),
    Pow,
    FloatDiv,
}

impl Checked {
    /// The integer operator `op` names, if it can fail, with `divisor`'s
    /// proof for `//` and `%`.
    pub fn named(op: &str, divisor: Divisor) -> Option<Self> {
        Some(match op {
            "+" => Self::Add,
            "-" => Self::Sub,
            "*" => Self::Mul,
            "//" => Self::FloorDiv(divisor),
            "%" => Self::Modulo(divisor),
            _ => return None,
        })
    }

    /// Whether the operation can overflow on operands of type `ty`. Among
    /// the divisions only signed `//` overflows, at `MIN // -1`; `MIN % -1`
    /// is 0.
    pub fn overflows(self, ty: &Ty) -> bool {
        match self {
            Self::Add | Self::Sub | Self::Mul | Self::Pow => true,
            Self::FloorDiv(proof) => {
                proof.may_be_minus_one() && matches!(ty, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64)
            }
            Self::Modulo(_) | Self::FloatDiv => false,
        }
    }

    /// Whether a zero divisor is an error.
    pub fn divides(self) -> bool {
        matches!(
            self,
            Self::FloorDiv(Divisor::Any) | Self::Modulo(Divisor::Any) | Self::FloatDiv
        )
    }

    /// Pops `T T` and pushes `T`.
    pub fn check(self, ty: &Ty, stack: &mut Vec<Ty>) -> Result<(), String> {
        let b = stack.pop().ok_or("checked arithmetic stack underflow")?;
        let a = stack.pop().ok_or("checked arithmetic stack underflow")?;
        let accepted = if self == Self::FloatDiv {
            a.is_float()
        } else {
            a.is_int()
        };
        if !accepted || a != b || a != *ty {
            return Err(format!(
                "checked arithmetic {self:?} does not accept ({a} {b})"
            ));
        }
        stack.push(a);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{FloatBits, NumericOp};
    use crate::op::Ty;

    #[test]
    fn representation_operations_accept_only_their_same_width_input() {
        for kind in [
            FloatBits::F32ToU32,
            FloatBits::U32ToF32,
            FloatBits::F64ToU64,
            FloatBits::U64ToF64,
        ] {
            let operation = NumericOp::Bits(kind);
            let (input, output) = kind.signature();
            let mut stack = vec![Ty::Bool, input.clone()];
            operation.check(&mut stack).unwrap();
            assert_eq!(stack, vec![Ty::Bool, output]);
            for wrong in [
                Ty::Bool,
                Ty::I32,
                Ty::I64,
                Ty::U32,
                Ty::U64,
                Ty::F32,
                Ty::F64,
            ] {
                if wrong != input {
                    assert!(operation.check(&mut vec![wrong]).is_err());
                }
            }
            assert!(operation.check(&mut Vec::new()).is_err());
        }
    }
}

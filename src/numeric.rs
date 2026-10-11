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

#[derive(Clone, Debug, PartialEq)]
pub enum NumericOp {
    /// `T -> T`. The minimum of a signed integer type traps as overflow.
    Abs,
    /// `T T -> T`. A NaN operand gives NaN.
    Min,
    Max,
    /// `T T -> T`. An integer result outside `T` and a negative integer
    /// exponent trap; floats follow the platform `pow`.
    Pow,
    /// `F -> F`.
    Sqrt,
    /// `F -> F`, or `F F -> F` for `Pow` and `Fmod`.
    Float(FloatOp),
    /// A list of numbers, or a shared reference to one when `borrowed`, to
    /// the checked sum of its elements in order, starting from zero.
    ListSum {
        element: Ty,
        borrowed: bool,
    },
}

impl NumericOp {
    /// Number of same-typed operands; `ListSum` takes its one list instead.
    fn operands(&self) -> usize {
        match self {
            Self::Min | Self::Max | Self::Pow | Self::Float(FloatOp::Pow | FloatOp::Fmod) => 2,
            _ => 1,
        }
    }

    pub fn check(&self, stack: &mut Vec<Ty>) -> Result<(), String> {
        if let Self::ListSum { element, borrowed } = self {
            let list = Ty::List(std::rc::Rc::new(element.clone()));
            let input = if *borrowed {
                Ty::Ref(std::rc::Rc::new(list), false)
            } else {
                list
            };
            if !element.is_numeric() || stack.pop() != Some(input) {
                return Err("sum requires a list of numbers".into());
            }
            stack.push(element.clone());
            return Ok(());
        }
        let ty = stack.pop().ok_or("numeric operation stack underflow")?;
        let accepted = match self {
            Self::Sqrt | Self::Float(_) => ty.is_float(),
            _ => ty.is_numeric(),
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

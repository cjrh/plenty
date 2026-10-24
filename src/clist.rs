//! Primitive column storage operations. Column iteration reads scalar values:
//! ordinary Plenty references address tagged slots, not packed native words.
use crate::op::Ty;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum ClistOp {
    New(Ty),
    WithCapacity(Ty),
    Reserve(Ty),
    Append(Ty),
    Len(Ty),
    Capacity(Ty),
    CopyRow(Ty),
    Get(Ty, usize),
    Set(Ty, usize),
    Column(Ty, usize),
    /// In-bounds packed column pointer and index -> scalar, lowered inline.
    Load(Ty),
}

pub fn row(ty: &Ty) -> &Ty {
    let Ty::Clist(row) = ty else { unreachable!() };
    row
}
pub fn field(ty: &Ty, index: usize) -> Ty {
    let Ty::Class(class) = row(ty) else {
        unreachable!()
    };
    class.get().fields[index].1.clone()
}
pub fn width(ty: &Ty) -> usize {
    match ty {
        Ty::Bool | Ty::I8 | Ty::U8 => 1,
        Ty::I16 | Ty::U16 => 2,
        Ty::I32 | Ty::U32 | Ty::F32 => 4,
        Ty::I64 | Ty::U64 | Ty::F64 => 8,
        _ => unreachable!("validated clist primitive"),
    }
}
impl ClistOp {
    pub fn owner(&self) -> &Ty {
        match self {
            Self::New(t)
            | Self::WithCapacity(t)
            | Self::Reserve(t)
            | Self::Append(t)
            | Self::Len(t)
            | Self::Capacity(t)
            | Self::CopyRow(t)
            | Self::Get(t, _)
            | Self::Set(t, _)
            | Self::Column(t, _) => t,
            Self::Load(_) => unreachable!("inline scalar load"),
        }
    }
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        if let Self::Load(ty) = self {
            return (vec![Ty::I64, Ty::I64], ty.clone());
        }
        let owner = self.owner();
        let reference = |mutable| Ty::Ref(Arc::new(owner.clone()), mutable);
        match self {
            Self::New(t) => (vec![], t.clone()),
            Self::WithCapacity(t) => (
                vec![Ty::I64],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
            Self::Reserve(_) => (
                vec![reference(true), Ty::I64],
                crate::sum::allocation_result(),
            ),
            Self::Append(_) => (
                vec![reference(true), row(owner).clone()],
                crate::sum::result(Ty::Unit, crate::sum::append_error(row(owner).clone())),
            ),
            Self::Len(_) | Self::Capacity(_) | Self::Column(..) => {
                (vec![reference(false)], Ty::I64)
            }
            Self::CopyRow(_) => (
                vec![reference(false), Ty::I64],
                crate::sum::option(row(owner).clone()),
            ),
            Self::Get(_, i) => (vec![reference(false), Ty::I64], field(owner, *i)),
            Self::Set(_, i) => (vec![reference(true), Ty::I64, field(owner, *i)], Ty::Unit),
            Self::Load(_) => unreachable!(),
        }
    }
    pub fn traps(&self) -> bool {
        matches!(self, Self::Get(..) | Self::Set(..))
    }
    pub fn opcode(&self) -> i64 {
        match self {
            Self::New(_) => 520,
            Self::WithCapacity(_) => 529,
            Self::Reserve(_) => 521,
            Self::Append(_) => 522,
            Self::Len(_) => 523,
            Self::Capacity(_) => 524,
            Self::CopyRow(_) => 525,
            Self::Get(..) => 526,
            Self::Set(..) => 527,
            Self::Column(..) => 528,
            Self::Load(_) => unreachable!("inline load"),
        }
    }
}

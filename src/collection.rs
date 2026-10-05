//! Statically typed collection operations shared by checking and native lowering.
use crate::op::Ty;

#[derive(Clone, Debug, PartialEq)]
pub enum CollectionOp {
    New(Ty),
    Insert(Ty), // private builder: collection, element (or key, value) -> collection
    Append(Ty), // persistent update of a list or set
    Put(Ty),    // persistent indexed update of a list or dictionary
    Get(Ty),
    Len(Ty),
    IterGet(Ty),
    Contains(Ty),
    Range,
    Values(Ty),
}

impl Ty {
    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            Self::List(_) | Self::Set(_) | Self::Dict(_, _) | Self::Range
        )
    }
    pub fn element(&self) -> Option<Ty> {
        match self {
            Self::List(t) | Self::Set(t) | Self::Dict(t, _) => Some((**t).clone()),
            Self::Range => Some(Self::I64),
            Self::Str => Some(Self::Str),
            _ => None,
        }
    }
    pub fn hashable(&self) -> bool {
        self.is_int() || matches!(self, Self::Bool | Self::Str)
    }
    /// Prefix encoding consumed by the native runtime, never a source-level type.
    pub fn descriptor(&self) -> String {
        match self {
            Self::I8 => "1".into(),
            Self::I16 => "2".into(),
            Self::I32 => "3".into(),
            Self::I64 => "4".into(),
            Self::U8 => "5".into(),
            Self::U16 => "6".into(),
            Self::U32 => "7".into(),
            Self::U64 => "8".into(),
            Self::Bool => "b".into(),
            Self::Str => "s".into(),
            Self::List(t) => format!("L{}", t.descriptor()),
            Self::Set(t) => format!("S{}", t.descriptor()),
            Self::Dict(k, v) => format!("D{}{}", k.descriptor(), v.descriptor()),
            Self::Range => "R".into(),
        }
    }
}

impl CollectionOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        use CollectionOp::*;
        match self {
            New(t) => (vec![], t.clone()),
            Insert(t) | Append(t) => {
                let mut args = vec![t.clone(), t.element().expect("collection element")];
                if let Ty::Dict(_, v) = t {
                    args.push((**v).clone());
                }
                (args, t.clone())
            }
            Put(t) => match t {
                Ty::List(v) => (vec![t.clone(), Ty::I64, (**v).clone()], t.clone()),
                Ty::Dict(k, v) => (vec![t.clone(), (**k).clone(), (**v).clone()], t.clone()),
                _ => unreachable!(),
            },
            Get(t) => match t {
                Ty::Dict(k, v) => (vec![t.clone(), (**k).clone()], (**v).clone()),
                _ => (vec![t.clone(), Ty::I64], t.element().unwrap()),
            },
            Len(t) => (vec![t.clone()], Ty::I64),
            IterGet(t) => (vec![t.clone(), Ty::I64], t.element().unwrap()),
            Contains(t) => (vec![t.element().unwrap(), t.clone()], Ty::Bool),
            Range => (vec![Ty::I64, Ty::I64, Ty::I64], Ty::Range),
            Values(t) => {
                let Ty::Dict(_, v) = t else { unreachable!() };
                (vec![t.clone()], Ty::List(v.clone()))
            }
        }
    }
    pub fn opcode(&self) -> i64 {
        match self {
            Self::New(_) => 0,
            Self::Insert(_) => 1,
            Self::Append(_) => 2,
            Self::Put(_) => 3,
            Self::Get(_) => 4,
            Self::Len(_) => 5,
            Self::IterGet(_) => 6,
            Self::Contains(_) => 7,
            Self::Range => 10,
            Self::Values(_) => 11,
        }
    }
}

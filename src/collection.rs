//! Statically typed collection operations shared by checking and native lowering.
use crate::op::Ty;

#[derive(Clone, Debug, PartialEq)]
pub enum CollectionOp {
    Copy(Ty),
    TryCopy(Ty),
    Next(Ty),
    New(Ty),
    TryNew(Ty),     // initial capacity -> Result[collection, AllocError]
    Insert(Ty),     // private builder: collection, element (or key, value) -> collection
    Append(Ty),     // exclusive in-place update of a list or set
    TryReserve(Ty), // exclusive capacity reservation -> Result[(), AllocError]
    TryInsert(Ty),  // exclusive fallible append/add/insert
    Put(Ty),        // exclusive in-place indexed update
    Get(Ty),
    Len(Ty),
    IterGet(Ty),
    IterTake(Ty),
    Contains(Ty),
    Range,
    Values(Ty),
    TextByteLen,
    TextAtByte,
    TextTryConcat,
    TextTryJoin,
}

impl Ty {
    pub fn uses_value_runtime(&self) -> bool {
        self.is_collection() || matches!(self, Self::Enum(_) | Self::Class(_) | Self::Generator(_))
    }
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
            Self::Generator(t) => Some((**t).clone()),
            _ => None,
        }
    }
    pub fn hashable(&self) -> bool {
        self.is_int() || matches!(self, Self::Bool | Self::Str)
    }
}

impl CollectionOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        use CollectionOp::*;
        match self {
            Copy(t) => (vec![t.clone()], t.clone()),
            TryCopy(t) => (
                vec![t.clone()],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
            Next(t) => (
                vec![t.clone()],
                crate::sum::option(t.element().expect("generator element")),
            ),
            TextByteLen => (vec![Ty::Str], Ty::I64),
            TextAtByte => (vec![Ty::Str, Ty::I64], Ty::Str),
            TextTryConcat => (
                vec![Ty::Str, Ty::Str],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            TextTryJoin => (
                vec![Ty::Str, Ty::List(std::rc::Rc::new(Ty::Str))],
                crate::sum::result(Ty::Str, crate::sum::alloc_error()),
            ),
            New(t) => (vec![], t.clone()),
            TryNew(t) => (
                vec![Ty::I64],
                crate::sum::result(t.clone(), crate::sum::alloc_error()),
            ),
            TryReserve(t) => (vec![t.clone(), Ty::I64], crate::sum::allocation_result()),
            TryInsert(t) => {
                let mut args = vec![t.clone(), t.element().expect("collection element")];
                if let Ty::Dict(_, v) = t {
                    args.push((**v).clone());
                }
                (args, crate::sum::allocation_result())
            }
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
            IterGet(t) | IterTake(t) => (vec![t.clone(), Ty::I64], t.element().unwrap()),
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
            Self::Copy(_) => 14,
            Self::TryCopy(_) => 33,
            Self::Next(_) => 24,
            Self::New(_) => 0,
            Self::TryNew(_) => 32,
            Self::Insert(_) => 1,
            Self::Append(_) => 2,
            Self::TryReserve(_) => 28,
            Self::TryInsert(_) => 29,
            Self::Put(_) => 3,
            Self::Get(_) => 4,
            Self::Len(_) => 5,
            Self::IterGet(_) => 6,
            Self::IterTake(_) => 15,
            Self::Contains(_) => 7,
            Self::Range => 10,
            Self::Values(_) => 11,
            Self::TextByteLen => 12,
            Self::TextAtByte => 13,
            Self::TextTryConcat => 34,
            Self::TextTryJoin => 35,
        }
    }
}

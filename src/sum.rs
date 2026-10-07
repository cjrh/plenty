//! Concrete nominal sum types and their checked operations.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct EnumType {
    pub restricted_storage: bool,
    // Names are unique within a compilation; builtin names include concrete args.
    pub name: String,
    pub variants: Vec<Variant>,
    pub depth: usize,
    pub affine: bool,
    pub copyable: bool,
    pub has_destructor: bool,
    pub managed: bool,
    pub inline_range: bool,
    /// Layout cache, excluded from nominal type equality and hashing.
    pub payload_bytes: std::cell::OnceCell<usize>,
}
impl PartialEq for EnumType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for EnumType {}
impl Hash for EnumType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}
impl EnumType {
    pub fn tuple(&self) -> bool {
        self.name.starts_with("tuple[")
    }
    /// Builtin sums have one payload and a binary discriminant at each level.
    pub fn inline(&self) -> bool {
        self.propagatable()
            || matches!(
                self.name.as_str(),
                "AllocError" | "ParseError" | "IoError" | "DataError" | "Failure"
            )
    }
    pub fn propagatable(&self) -> bool {
        self.name.starts_with("Option[") || self.name.starts_with("Result[")
    }
    pub fn is_option(&self) -> bool {
        self.name.starts_with("Option[")
    }
    /// Only an explicitly declared Result[..., Failure] erases propagated errors.
    pub fn discards_error(&self) -> bool {
        self.name.starts_with("Result[")
            && matches!(self.variants[1].fields.as_slice(), [Ty::Enum(t)] if t.name == "Failure")
    }
}

/// A payload-free marker for callers that deliberately discard error details.
pub fn failure() -> Ty {
    Ty::Enum(Rc::new(EnumType {
        restricted_storage: false,
        name: "Failure".into(),
        variants: vec![Variant {
            name: "Unspecified".into(),
            fields: vec![],
        }],
        depth: 1,
        affine: false,
        copyable: true,
        has_destructor: false,
        managed: false,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
    }))
}

/// An allocation error must itself be constructible without allocating.
pub fn alloc_error() -> Ty {
    Ty::Enum(Rc::new(EnumType {
        restricted_storage: false,
        name: "AllocError".into(),
        variants: ["OutOfMemory", "CapacityOverflow"]
            .into_iter()
            .map(|name| Variant {
                name: name.into(),
                fields: vec![],
            })
            .collect(),
        depth: 1,
        affine: false,
        copyable: true,
        has_destructor: false,
        managed: false,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
    }))
}

pub fn allocation_result() -> Ty {
    result(Ty::Unit, alloc_error())
}

pub fn data_error() -> Ty {
    let Ty::Enum(template) = result(Ty::Unit, alloc_error()) else {
        unreachable!()
    };
    let mut ty = (*template).clone();
    ty.name = "DataError".into();
    ty.variants[0] = Variant {
        name: "InvalidUtf8".into(),
        fields: vec![],
    };
    ty.variants[1].name = "Allocation".into();
    Ty::Enum(Rc::new(ty))
}

pub fn io_error() -> Ty {
    let Ty::Enum(template) = result(Ty::I32, data_error()) else {
        unreachable!()
    };
    let mut ty = (*template).clone();
    ty.name = "IoError".into();
    ty.variants[0].name = "System".into();
    ty.variants[1].name = "Data".into();
    Ty::Enum(Rc::new(ty))
}

pub fn parse_error() -> Ty {
    let Ty::Enum(template) = alloc_error() else {
        unreachable!()
    };
    let mut ty = (*template).clone();
    ty.name = "ParseError".into();
    ty.variants[0].name = "Invalid".into();
    ty.variants[1].name = "OutOfRange".into();
    Ty::Enum(Rc::new(ty))
}
#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Ty>,
}

/// Structural products reuse the checked record storage and field operations.
pub fn tuple(fields: Vec<Ty>) -> Ty {
    Ty::Enum(Rc::new(EnumType {
        name: format!(
            "tuple[{}]",
            fields
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        depth: 1 + fields.iter().map(Ty::layout_depth).max().unwrap_or(0),
        affine: fields.iter().any(Ty::affine),
        copyable: fields.iter().all(Ty::can_copy),
        has_destructor: fields.iter().any(Ty::has_destructor),
        restricted_storage: fields.iter().any(Ty::restricted_storage),
        managed: true,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
        variants: vec![Variant {
            name: String::new(),
            fields,
        }],
    }))
}

pub fn option(element: Ty) -> Ty {
    Ty::Enum(Rc::new(EnumType {
        restricted_storage: element.restricted_storage(),
        name: format!("Option[{element}]"),
        depth: 1 + element.layout_depth(),
        affine: element.affine(),
        copyable: element.can_copy(),
        has_destructor: element.has_destructor(),
        managed: element.managed(),
        inline_range: element.has_inline_range(),
        payload_bytes: std::cell::OnceCell::new(),
        variants: vec![
            Variant {
                name: "Nothing".into(),
                fields: vec![],
            },
            Variant {
                name: "Some".into(),
                fields: vec![element],
            },
        ],
    }))
}
pub fn result(ok: Ty, error: Ty) -> Ty {
    Ty::Enum(Rc::new(EnumType {
        restricted_storage: ok.restricted_storage() || error.restricted_storage(),
        name: format!("Result[{ok}, {error}]"),
        depth: 1 + ok.layout_depth().max(error.layout_depth()),
        affine: ok.affine() || error.affine(),
        copyable: ok.can_copy() && error.can_copy(),
        has_destructor: ok.has_destructor() || error.has_destructor(),
        managed: ok.managed() || error.managed(),
        inline_range: ok.has_inline_range() || error.has_inline_range(),
        payload_bytes: std::cell::OnceCell::new(),
        variants: vec![
            Variant {
                name: "Ok".into(),
                fields: vec![ok],
            },
            Variant {
                name: "Err".into(),
                fields: vec![error],
            },
        ],
    }))
}

#[derive(Clone, Debug, PartialEq)]
pub enum EnumOp {
    New(Rc<EnumType>, usize),
    TryNew(Rc<EnumType>, usize),
    Unwrap(Rc<EnumType>),
    Tag(Rc<EnumType>),
    Field(Rc<EnumType>, usize, usize),
    Take(Rc<EnumType>, usize, usize),
}
impl EnumOp {
    pub fn signature(&self) -> Option<(Vec<Ty>, Ty)> {
        Some(match self {
            Self::Unwrap(t) if t.propagatable() => (
                vec![Ty::Enum(t.clone())],
                t.variants[usize::from(t.is_option())].fields[0].clone(),
            ),
            Self::Unwrap(_) => return None,
            Self::New(t, tag) => (t.variants.get(*tag)?.fields.clone(), Ty::Enum(t.clone())),
            Self::TryNew(t, tag) => (
                t.variants.get(*tag)?.fields.clone(),
                result(Ty::Enum(t.clone()), alloc_error()),
            ),
            Self::Tag(t) => (vec![Ty::Enum(t.clone())], Ty::I64),
            Self::Field(t, tag, field) | Self::Take(t, tag, field) => (
                vec![Ty::Enum(t.clone())],
                t.variants.get(*tag)?.fields.get(*field)?.clone(),
            ),
        })
    }
}

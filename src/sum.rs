//! Concrete nominal sum types and their checked operations.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct EnumType {
    pub facts: std::cell::OnceCell<crate::type_facts::Facts>,
    pub restricted_storage: bool,
    // Names are unique within a compilation; builtin names include concrete args.
    pub name: String,
    pub variants: Vec<Variant>,
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
impl crate::nominal::Nominal<EnumType> {
    pub fn tuple(&self) -> bool {
        self.name.starts_with("tuple[")
    }
    pub fn inline(&self) -> bool {
        self.propagatable()
            || matches!(
                self.name.as_str(),
                "AllocError"
                    | "ParseError"
                    | "IoError"
                    | "DataError"
                    | "Failure"
                    | "CStrError"
                    | "LoadError"
            )
    }
    pub fn propagatable(&self) -> bool {
        self.name.starts_with("Option[") || self.name.starts_with("Result[")
    }
    pub fn is_option(&self) -> bool {
        self.name.starts_with("Option[")
    }
    pub fn discards_error(&self) -> bool {
        self.name.starts_with("Result[")
            && matches!(self.local().variants[1].fields.as_slice(), [Ty::Enum(t)] if t.name == "Failure")
    }
}

/// A payload-free marker for callers that deliberately discard error details.
pub fn failure() -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: false,
        name: "Failure".into(),
        variants: vec![Variant {
            name: "Unspecified".into(),
            fields: vec![],
        }],
        facts: std::cell::OnceCell::new(),
        managed: false,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
    }))
}

/// An allocation error must itself be constructible without allocating.
pub fn alloc_error() -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: false,
        name: "AllocError".into(),
        variants: ["OutOfMemory", "CapacityOverflow"]
            .into_iter()
            .map(|name| Variant {
                name: name.into(),
                fields: vec![],
            })
            .collect(),
        facts: std::cell::OnceCell::new(),
        managed: false,
        inline_range: false,
        payload_bytes: std::cell::OnceCell::new(),
    }))
}

pub fn allocation_result() -> Ty {
    result(Ty::Unit, alloc_error())
}

/// Loader failures are fieldless markers with a three-bit inline discriminant.
pub fn load_error() -> Ty {
    let Ty::Enum(template) = alloc_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "LoadError".into();
    ty.variants = [
        "OutOfMemory",
        "CapacityOverflow",
        "InvalidPath",
        "OpenFailed",
        "InvalidSymbol",
        "MissingSymbol",
        "IncompatibleContract",
    ]
    .into_iter()
    .map(|name| Variant {
        name: name.into(),
        fields: vec![],
    })
    .collect();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn data_error() -> Ty {
    let Ty::Enum(template) = result(Ty::Unit, alloc_error()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "DataError".into();
    ty.variants[0] = Variant {
        name: "InvalidUtf8".into(),
        fields: vec![],
    };
    ty.variants[1].name = "Allocation".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

/// Conversion failures before a native C-string call. These errors allocate nothing.
pub fn c_str_error() -> Ty {
    let Ty::Enum(template) = data_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "CStrError".into();
    ty.variants[0].name = "EmbeddedNul".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn io_error() -> Ty {
    let Ty::Enum(template) = result(Ty::I32, data_error()) else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "IoError".into();
    ty.variants[0].name = "System".into();
    ty.variants[1].name = "Data".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}

pub fn parse_error() -> Ty {
    let Ty::Enum(template) = alloc_error() else {
        unreachable!()
    };
    let mut ty = (*template.get()).clone();
    ty.facts = std::cell::OnceCell::new();
    ty.name = "ParseError".into();
    ty.variants[0].name = "Invalid".into();
    ty.variants[1].name = "OutOfRange".into();
    Ty::Enum(crate::nominal::Nominal::new(ty))
}
#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Ty>,
}

/// Structural products reuse the checked record storage and field operations.
pub fn tuple(fields: Vec<Ty>) -> Ty {
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        name: format!(
            "tuple[{}]",
            fields
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        facts: std::cell::OnceCell::new(),
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
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: element.restricted_storage(),
        name: format!("Option[{element}]"),
        facts: std::cell::OnceCell::new(),
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
    Ty::Enum(crate::nominal::Nominal::new(EnumType {
        restricted_storage: ok.restricted_storage() || error.restricted_storage(),
        name: format!("Result[{ok}, {error}]"),
        facts: std::cell::OnceCell::new(),
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
    New(crate::nominal::Nominal<EnumType>, usize),
    TryNew(crate::nominal::Nominal<EnumType>, usize),
    Unwrap(crate::nominal::Nominal<EnumType>),
    Tag(crate::nominal::Nominal<EnumType>),
    TagRef(crate::nominal::Nominal<EnumType>, bool),
    FieldRef(crate::nominal::Nominal<EnumType>, usize, usize, bool),
    Field(crate::nominal::Nominal<EnumType>, usize, usize),
    Take(crate::nominal::Nominal<EnumType>, usize, usize),
}
impl EnumOp {
    pub fn signature(&self) -> Option<(Vec<Ty>, Ty)> {
        Some(match self {
            Self::Unwrap(t) if t.propagatable() => (
                vec![Ty::Enum(t.clone())],
                t.get().variants[usize::from(t.is_option())].fields[0].clone(),
            ),
            Self::Unwrap(_) => return None,
            Self::New(t, tag) => (
                t.get().variants.get(*tag)?.fields.clone(),
                Ty::Enum(t.clone()),
            ),
            Self::TryNew(t, tag) => (
                t.get().variants.get(*tag)?.fields.clone(),
                result(Ty::Enum(t.clone()), alloc_error()),
            ),
            Self::Tag(t) => (vec![Ty::Enum(t.clone())], Ty::I64),
            Self::TagRef(t, mutable) => (
                vec![Ty::Ref(Rc::new(Ty::Enum(t.clone())), *mutable)],
                Ty::I64,
            ),
            Self::FieldRef(t, tag, field, mutable) => (
                vec![Ty::Ref(Rc::new(Ty::Enum(t.clone())), *mutable)],
                Ty::Ref(
                    Rc::new(t.get().variants.get(*tag)?.fields.get(*field)?.clone()),
                    *mutable,
                ),
            ),
            Self::Field(t, tag, field) | Self::Take(t, tag, field) => (
                vec![Ty::Enum(t.clone())],
                t.get().variants.get(*tag)?.fields.get(*field)?.clone(),
            ),
        })
    }
}

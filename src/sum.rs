//! Concrete nominal sum types and their checked operations.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct EnumType {
    // Names are unique within a compilation; builtin names include concrete args.
    pub name: String,
    pub variants: Vec<Variant>,
    pub depth: usize,
    pub affine: bool,
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
#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Ty>,
}

pub fn option(element: Ty) -> Ty {
    Ty::Enum(Rc::new(EnumType {
        name: format!("Option[{element}]"),
        depth: 1 + element.layout_depth(),
        affine: element.affine(),
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
        name: format!("Result[{ok}, {error}]"),
        depth: 1 + ok.layout_depth().max(error.layout_depth()),
        affine: ok.affine() || error.affine(),
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
    Tag(Rc<EnumType>),
    Field(Rc<EnumType>, usize, usize),
}
impl EnumOp {
    pub fn signature(&self) -> Option<(Vec<Ty>, Ty)> {
        Some(match self {
            Self::New(t, tag) => (t.variants.get(*tag)?.fields.clone(), Ty::Enum(t.clone())),
            Self::Tag(t) => (vec![Ty::Enum(t.clone())], Ty::I64),
            Self::Field(t, tag, field) => (
                vec![Ty::Enum(t.clone())],
                t.variants.get(*tag)?.fields.get(*field)?.clone(),
            ),
        })
    }
}

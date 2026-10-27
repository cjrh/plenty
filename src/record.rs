//! Nominal fixed-layout classes and their native operations.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ClassType {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
    pub destructor: Option<String>,
    pub fallible_init: bool,
    /// Declared `@copy`: instances copy on ordinary use instead of moving.
    /// The declaration check guarantees every field copies and there is no
    /// `__del__`; see book/src/design/language/03-types/classes.md.
    pub copy: bool,
    /// Inline field storage size; excluded from nominal equality.
    pub bytes: std::sync::OnceLock<usize>,
    /// Whether an instance needs cleanup; excluded from nominal equality.
    pub managed: std::sync::OnceLock<bool>,
}
impl ClassType {
    /// A class with `__del__` keeps a state word before its fields: whether
    /// initialization completed, and how many observers borrow it.
    pub fn state_bytes(&self) -> usize {
        if self.destructor.is_some() { 16 } else { 0 }
    }
}
impl PartialEq for ClassType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for ClassType {}
impl Hash for ClassType {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.name.hash(h);
    }
}
pub fn method(class: &str, method: &str) -> String {
    format!("__plenty_class_{}.{}", class, method)
}
/// Whether `symbol` came from `method`. Source names cannot start this way.
pub fn is_method(symbol: &str) -> bool {
    symbol.starts_with("__plenty_class_")
}
/// A method symbol as `Class.method`, for diagnostics.
pub fn method_display(symbol: &str) -> &str {
    symbol.strip_prefix("__plenty_class_").unwrap_or(symbol)
}
#[derive(Clone, Debug, PartialEq)]
pub enum ClassOp {
    /// Zeroed inline storage, before `__init__` initializes the fields.
    New(crate::nominal::Nominal<ClassType>),
    ArmDrop(crate::nominal::Nominal<ClassType>),
    Field(crate::nominal::Nominal<ClassType>, usize),
    FieldRef(crate::nominal::Nominal<ClassType>, usize, bool),
}
impl ClassOp {
    pub fn signature(&self) -> Option<(Vec<Ty>, Ty)> {
        Some(match self {
            Self::ArmDrop(t) => (
                vec![Ty::Ref(Arc::new(Ty::Class(t.clone())), true)],
                Ty::Unit,
            ),
            Self::New(t) => (vec![], Ty::Class(t.clone())),
            Self::Field(t, i) => (
                vec![Ty::Class(t.clone())],
                t.get().fields.get(*i)?.1.clone(),
            ),
            Self::FieldRef(t, i, mutable) => (
                vec![Ty::Ref(Arc::new(Ty::Class(t.clone())), *mutable)],
                Ty::Ref(Arc::new(t.get().fields.get(*i)?.1.clone()), *mutable),
            ),
        })
    }
}

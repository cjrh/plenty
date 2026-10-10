//! Nominal fixed-layout classes and their native operations.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct ClassType {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
    pub destructor: Option<String>,
    pub fallible_init: bool,
    /// Inline field storage size; excluded from nominal equality.
    pub bytes: std::cell::OnceCell<usize>,
    /// Whether an instance needs cleanup; excluded from nominal equality.
    pub managed: std::cell::OnceCell<bool>,
}
impl ClassType {
    /// A class with `__del__` keeps a state word before its fields: whether
    /// initialization completed, and how many observers borrow it.
    pub fn state_bytes(&self) -> usize {
        if self.destructor.is_some() {
            16
        } else {
            0
        }
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
            Self::ArmDrop(t) => (vec![Ty::Ref(Rc::new(Ty::Class(t.clone())), true)], Ty::Unit),
            Self::New(t) => (vec![], Ty::Class(t.clone())),
            Self::Field(t, i) => (
                vec![Ty::Class(t.clone())],
                t.get().fields.get(*i)?.1.clone(),
            ),
            Self::FieldRef(t, i, mutable) => (
                vec![Ty::Ref(Rc::new(Ty::Class(t.clone())), *mutable)],
                Ty::Ref(Rc::new(t.get().fields.get(*i)?.1.clone()), *mutable),
            ),
        })
    }
}

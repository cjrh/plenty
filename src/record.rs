//! Nominal fixed-layout classes and their native operations.
use crate::op::Ty;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct ClassType {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
    pub destructor: Option<String>,
    pub depth: usize,
    pub copyable: bool,
    pub has_destructor: bool,
    pub fallible_init: bool,
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
    New(Rc<ClassType>),
    TryNew(Rc<ClassType>),
    ArmDrop(Rc<ClassType>),
    Field(Rc<ClassType>, usize),
    FieldRef(Rc<ClassType>, usize, bool),
}
impl ClassOp {
    pub fn signature(&self) -> Option<(Vec<Ty>, Ty)> {
        Some(match self {
            Self::ArmDrop(t) => (vec![Ty::Ref(Rc::new(Ty::Class(t.clone())), true)], Ty::Unit),
            Self::New(t) => (vec![], Ty::Class(t.clone())),
            Self::TryNew(t) => (
                vec![],
                crate::sum::result(Ty::Class(t.clone()), crate::sum::alloc_error()),
            ),
            Self::Field(t, i) => (vec![Ty::Class(t.clone())], t.fields.get(*i)?.1.clone()),
            Self::FieldRef(t, i, mutable) => (
                vec![Ty::Ref(Rc::new(Ty::Class(t.clone())), *mutable)],
                Ty::Ref(Rc::new(t.fields.get(*i)?.1.clone()), *mutable),
            ),
        })
    }
}

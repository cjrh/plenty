//! `Box[T]`: one owned heap value. Recursive types reach themselves through
//! boxes, so every other value can keep its storage inline.
use crate::op::Ty;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum BoxOp {
    /// Move a value into a new box; allocation failure drops the value.
    New(Ty),
    /// Move the content out and free the box.
    Take(Ty),
    /// Borrow the content through a borrowed box.
    Ref(Ty, bool),
}
impl BoxOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        match self {
            Self::New(t) => (
                vec![t.clone()],
                crate::sum::result(Ty::Box(Rc::new(t.clone())), crate::sum::alloc_error()),
            ),
            Self::Take(t) => (vec![Ty::Box(Rc::new(t.clone()))], t.clone()),
            Self::Ref(t, mutable) => (
                vec![Ty::Ref(Rc::new(Ty::Box(Rc::new(t.clone()))), *mutable)],
                Ty::Ref(Rc::new(t.clone()), *mutable),
            ),
        }
    }
}

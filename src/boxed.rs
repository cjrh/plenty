//! `Box[T]`: one owned heap value. Recursive types reach themselves through
//! boxes, so every other value can keep its storage inline. `Shared[T]` uses
//! the same counted heap record with several owners, so it never moves its
//! content out and never lends it mutably.
use crate::op::Ty;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum BoxOp {
    /// Move a value into a new box; allocation failure drops the value.
    New(Ty),
    /// Move the content out and free the box.
    Take(Ty),
    /// Borrow the content through a borrowed box.
    Ref(Ty, bool),
    /// Move a value into a new shared owner; allocation failure drops the value.
    Share(Ty),
    /// Borrow the content shared through a borrowed shared owner.
    SharedRef(Ty),
}
impl BoxOp {
    pub fn signature(&self) -> (Vec<Ty>, Ty) {
        match self {
            Self::New(t) => (
                vec![t.clone()],
                crate::sum::result(Ty::Box(Arc::new(t.clone())), crate::sum::alloc_error()),
            ),
            Self::Take(t) => (vec![Ty::Box(Arc::new(t.clone()))], t.clone()),
            Self::Ref(t, mutable) => (
                vec![Ty::Ref(Arc::new(Ty::Box(Arc::new(t.clone()))), *mutable)],
                Ty::Ref(Arc::new(t.clone()), *mutable),
            ),
            Self::Share(t) => (
                vec![t.clone()],
                crate::sum::result(Ty::Shared(Arc::new(t.clone())), crate::sum::alloc_error()),
            ),
            Self::SharedRef(t) => (
                vec![Ty::Ref(Arc::new(Ty::Shared(Arc::new(t.clone()))), false)],
                Ty::Ref(Arc::new(t.clone()), false),
            ),
        }
    }
}

//! Concrete, owner-local closure environments. Code is statically selected;
//! capture storage is part of the value, never an implicit heap allocation.
use crate::op::{CallableSig, Ty};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ClosureType {
    pub name: String,
    pub signature: CallableSig,
    pub captures: Vec<(String, Ty)>,
    pub writable: Vec<bool>,
    pub mutable: bool,
}

impl ClosureType {
    pub fn parameter(&self, index: usize) -> Ty {
        let ty = &self.captures[index].1;
        if matches!(ty, Ty::Ref(..)) {
            ty.clone()
        } else {
            Ty::Ref(std::rc::Rc::new(ty.clone()), self.writable[index])
        }
    }
    pub fn bytes(&self) -> usize {
        16 + self
            .captures
            .iter()
            .map(|(_, t)| t.slot_bytes())
            .sum::<usize>()
    }

    pub fn offset(&self, index: usize) -> usize {
        16 + self.captures[..index]
            .iter()
            .map(|(_, t)| t.slot_bytes())
            .sum::<usize>()
    }
}

//! Concrete, owner-local closure environments. Code is statically selected;
//! capture storage is part of the value, never an implicit heap allocation.
use crate::op::{CallableSig, Ty};
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct ClosureType {
    pub name: String,
    pub signature: CallableSig,
    pub captures: Vec<(String, Ty)>,
    pub writable: Vec<bool>,
    pub mutable: bool,
    pub depth: usize,
    pub borrowed: bool,
    bytes: usize,
}

// A concrete expression has one environment identity. Hashing its entire
// capture graph would repeatedly expand shared nested environments.
impl PartialEq for ClosureType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && (!self.name.is_empty() || self.signature == other.signature)
    }
}
impl Eq for ClosureType {}
impl Hash for ClosureType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        if self.name.is_empty() {
            self.signature.hash(state);
        }
    }
}

impl ClosureType {
    pub fn new(
        name: String,
        signature: CallableSig,
        captures: Vec<(String, Ty)>,
        writable: Vec<bool>,
    ) -> Result<Self, String> {
        let bytes = captures.iter().try_fold(16usize, |size, (_, ty)| {
            size.checked_add(ty.slot_bytes())
                .filter(|n| *n <= i32::MAX as usize)
                .ok_or_else(|| {
                    "closure environment exceeds the native stack-layout limit".to_string()
                })
        })?;
        let depth = 1 + captures
            .iter()
            .map(|(_, ty)| ty.layout_depth())
            .max()
            .unwrap_or(0);
        let borrowed = captures.iter().any(|(_, ty)| ty.contains_reference());
        let mutable = writable.iter().any(|m| *m);
        Ok(Self {
            name,
            signature,
            captures,
            writable,
            mutable,
            depth,
            borrowed,
            bytes,
        })
    }
    pub fn parameter(&self, index: usize) -> Ty {
        let ty = &self.captures[index].1;
        if matches!(ty, Ty::Ref(..)) {
            ty.clone()
        } else {
            Ty::Ref(std::rc::Rc::new(ty.clone()), self.writable[index])
        }
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn offset(&self, index: usize) -> usize {
        16 + self.captures[..index]
            .iter()
            .map(|(_, t)| t.slot_bytes())
            .sum::<usize>()
    }
}

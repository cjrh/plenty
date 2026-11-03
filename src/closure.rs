//! Concrete, owner-local closure environments. Code is statically selected;
//! capture storage is part of the value, never an implicit heap allocation.
use crate::generator::{Measured, NESTING_LIMIT};
use crate::op::{CallableSig, Ty};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct ClosureType {
    pub name: String,
    /// Diagnostic origin only; nominal identity and linking still use `name`.
    pub location: Option<Arc<str>>,
    pub signature: CallableSig,
    pub captures: Vec<(String, Ty)>,
    pub writable: Vec<bool>,
    pub mutable: bool,
    pub once: bool,
    pub depth: usize,
    pub borrowed: bool,
    layout: OnceLock<Measured>,
}

// A concrete expression has one environment identity. Hashing its entire
// capture graph would repeatedly expand shared nested environments.
impl PartialEq for ClosureType {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && (!self.name.is_empty()
                || (self.signature == other.signature && self.once == other.once))
    }
}
impl Eq for ClosureType {}
impl Hash for ClosureType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        if self.name.is_empty() {
            self.signature.hash(state);
            self.once.hash(state);
        }
    }
}

impl ClosureType {
    pub fn new(
        name: String,
        location: Option<Arc<str>>,
        signature: CallableSig,
        captures: Vec<(String, Ty)>,
        writable: Vec<bool>,
        once: bool,
    ) -> Result<Self, String> {
        let layout = OnceLock::new();
        // Captured frame bodies are finalized by the normal lowering work queue.
        // Defer only their dependent sizes, avoiding recursive body lowering.
        // A size known now is never measured, so no nesting limit applies.
        if !captures.iter().any(|(_, ty)| ty.contains_generator_frame()) {
            let bytes = captures.iter().try_fold(16usize, |size, (_, ty)| {
                size.checked_add(ty.slot_bytes())
                    .filter(|n| *n <= i32::MAX as usize)
                    .ok_or_else(|| {
                        "closure environment exceeds the native stack-layout limit".to_string()
                    })
            })?;
            layout
                .set(Measured { bytes, reach: 0 })
                .expect("new environment layout");
        }
        let depth = 1 + captures
            .iter()
            .map(|(_, ty)| ty.layout_depth())
            .max()
            .unwrap_or(0);
        let borrowed = captures.iter().any(|(_, ty)| ty.contains_reference());
        let mutable = writable.iter().any(|m| *m);
        Ok(Self {
            name,
            location,
            signature,
            captures,
            writable,
            mutable,
            once,
            depth,
            borrowed,
            layout,
        })
    }
    pub fn parameter(&self, index: usize) -> Ty {
        let ty = &self.captures[index].1;
        if self.once || matches!(ty, Ty::Ref(..)) {
            ty.clone()
        } else {
            Ty::Ref(std::sync::Arc::new(ty.clone()), self.writable[index])
        }
    }
    pub fn bytes(&self) -> usize {
        self.layout.get().expect("resolved closure layout").bytes
    }

    pub fn layout(&self, active: &mut Vec<String>) -> Result<Measured, String> {
        if let Some(cached) = self.layout.get().and_then(|m| m.at_depth(active.len())) {
            return Ok(cached);
        }
        if active.contains(&self.name) {
            return Err("recursive inline closure layout has no finite size".into());
        }
        if active.len() >= NESTING_LIMIT {
            return Err("inline closure nesting exceeds the implementation limit of 64".into());
        }
        active.push(self.name.clone());
        let mut reach = 0;
        let bytes = self.captures.iter().try_fold(16usize, |size, (_, ty)| {
            let capture = crate::generator::measure(ty, active)?;
            reach = reach.max(capture.reach);
            size.checked_add(16 + capture.bytes)
                .filter(|n| *n <= i32::MAX as usize)
                .ok_or_else(|| {
                    "closure environment exceeds the native stack-layout limit".to_string()
                })
        })?;
        active.pop();
        // Threads measuring the same environment at once agree.
        Ok(*self.layout.get_or_init(|| Measured {
            bytes,
            reach: reach + 1,
        }))
    }

    pub fn offset(&self, index: usize) -> usize {
        16 + self.captures[..index]
            .iter()
            .map(|(_, t)| t.slot_bytes())
            .sum::<usize>()
    }
}

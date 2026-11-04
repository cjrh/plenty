//! `Box[T]`: one owned heap value. Recursive types reach themselves through
//! boxes, so every other value can keep its storage inline. `Shared[T]` uses
//! the same counted heap record with several owners, so it never moves its
//! content out and never lends it mutably.
use crate::op::Ty;
use std::sync::Arc;

/// Fully resolved public interface; signatures exclude the erased receiver.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProtocolType {
    pub name: String,
    pub arguments: Vec<Ty>,
    pub methods: Vec<ProtocolMethod>,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProtocolMethod {
    pub name: String,
    pub mutable: bool,
    pub signature: crate::op::CallableSig,
}
impl ProtocolType {
    pub fn types(&self) -> impl Iterator<Item = &Ty> {
        self.arguments
            .iter()
            .chain(self.methods.iter().flat_map(|method| {
                method
                    .signature
                    .inputs
                    .iter()
                    .chain(method.signature.output.iter())
            }))
    }
    pub(crate) fn map_types(&self, mut map: impl FnMut(&Ty) -> Ty) -> Self {
        Self {
            name: self.name.clone(),
            arguments: self.arguments.iter().map(&mut map).collect(),
            methods: self
                .methods
                .iter()
                .map(|method| ProtocolMethod {
                    name: method.name.clone(),
                    mutable: method.mutable,
                    signature: crate::op::CallableSig {
                        inputs: method.signature.inputs.iter().map(&mut map).collect(),
                        output: method.signature.output.as_ref().map(&mut map),
                    },
                })
                .collect(),
        }
    }
    pub fn identity_name(&self) -> String {
        if self.arguments.is_empty() {
            return format!("Box[{}]", self.name);
        }
        format!(
            "Box[{}[{}]]",
            self.name,
            self.arguments
                .iter()
                .map(Ty::identity_name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
impl std::fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Box[{}", self.name)?;
        if !self.arguments.is_empty() {
            f.write_str("[")?;
            for (i, argument) in self.arguments.iter().enumerate() {
                if i != 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{argument}")?;
            }
            f.write_str("]")?;
        }
        f.write_str("]")
    }
}

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

/// These owners carry obligations that must remain visible to static analysis.
/// An already-erased callback could conceal the same owners in its captures.
pub fn erasure_barrier(source: &Ty) -> bool {
    let mut pending = vec![source.clone()];
    let mut seen = Vec::new();
    while let Some(ty) = pending.pop() {
        if seen.contains(&ty) {
            continue;
        }
        if matches!(ty, Ty::Executor | Ty::Future(_) | Ty::BoxedCallable(_)) {
            return true;
        }
        crate::threading::contained(&ty, &mut pending);
        seen.push(ty);
    }
    false
}

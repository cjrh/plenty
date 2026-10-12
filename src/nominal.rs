//! Compilation-owned nominal definitions. Stored edges are weak; escaped handles
//! retain the table. Neither recursive declarations nor failed compilation leak
//! an Arc cycle, and looking up a definition never expands its children.
use crate::{op::Ty, record::ClassType, sum::EnumType};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};
use std::sync::{Mutex, OnceLock};

type Slot<T> = Arc<OnceLock<Arc<T>>>;

#[derive(Default)]
pub struct Types {
    classes: Mutex<Vec<Slot<ClassType>>>,
    enums: Mutex<Vec<Slot<EnumType>>>,
    facts: Mutex<std::collections::HashMap<String, crate::type_facts::Facts>>,
}

enum Link<T> {
    Local(Arc<T>),
    Stored(Arc<T>),
    Owned(Arc<Types>, Weak<OnceLock<Arc<T>>>),
    Weak(Weak<Types>, Weak<OnceLock<Arc<T>>>),
}

pub struct Nominal<T> {
    pub name: String,
    /// User-facing spelling, excluded from identity and type-table lookup.
    display_name: Option<Arc<str>>,
    link: Link<T>,
}

pub trait Definition: Clone {
    fn name(&self) -> &str;
    fn weaken(&mut self);
    fn cached_facts(&self) -> Option<crate::type_facts::Facts> {
        None
    }
    fn cache_facts(&self, _facts: crate::type_facts::Facts) {}
}

impl<T: Definition> Nominal<T> {
    pub fn new(value: T) -> Self {
        Self {
            name: value.name().into(),
            display_name: None,
            link: Link::Local(Arc::new(value)),
        }
    }
    pub fn with_display_name(mut self, name: String) -> Self {
        self.display_name = (name != self.name).then(|| name.into());
        self
    }
    pub fn display_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }
    pub fn get(&self) -> Arc<T> {
        self.try_get()
            .expect("nominal definition must be finalized before use")
    }
    /// Builtin structural sums are complete local definitions, never table slots.
    pub fn local(&self) -> &T {
        match &self.link {
            Link::Local(t) | Link::Stored(t) => t,
            _ => panic!("expected a builtin structural definition"),
        }
    }
    pub fn try_get(&self) -> Option<Arc<T>> {
        match &self.link {
            Link::Local(t) | Link::Stored(t) => Some(t.clone()),
            Link::Owned(_, slot) | Link::Weak(_, slot) => slot.upgrade()?.get().cloned(),
        }
    }
    pub fn define(&self, mut value: T) {
        let (Link::Owned(_, slot) | Link::Weak(_, slot)) = &self.link else {
            panic!("only reserved nominal definitions can be finalized")
        };
        value.weaken();
        assert!(slot.upgrade().unwrap().set(Arc::new(value)).is_ok());
        if let Some(owner) = self.owner() {
            owner.facts.lock().unwrap().clear();
        }
    }
    fn owner(&self) -> Option<Arc<Types>> {
        match &self.link {
            Link::Owned(owner, _) => Some(owner.clone()),
            Link::Weak(owner, _) => owner.upgrade(),
            _ => None,
        }
    }
    pub fn cached_facts(&self) -> Option<crate::type_facts::Facts> {
        if let Link::Local(t) | Link::Stored(t) = &self.link {
            return t.cached_facts();
        }
        self.owner()?.facts.lock().unwrap().get(&self.name).copied()
    }
    pub fn cache_facts(&self, facts: crate::type_facts::Facts) {
        if let Link::Local(t) | Link::Stored(t) = &self.link {
            t.cache_facts(facts);
            return;
        }
        if let Some(owner) = self.owner() {
            owner.facts.lock().unwrap().insert(self.name.clone(), facts);
        }
    }
    fn weaken(&self) -> Self {
        let link = match &self.link {
            Link::Owned(owner, slot) => Link::Weak(Arc::downgrade(owner), slot.clone()),
            Link::Weak(owner, slot) => Link::Weak(owner.clone(), slot.clone()),
            Link::Local(value) | Link::Stored(value) => {
                let mut value = (**value).clone();
                value.weaken();
                Link::Stored(Arc::new(value))
            }
        };
        Self {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            link,
        }
    }
}
impl<T: Definition> Clone for Nominal<T> {
    fn clone(&self) -> Self {
        let link = match &self.link {
            Link::Local(t) => Link::Local(t.clone()),
            // Cloning fields out of the table promotes all weak nominal edges.
            Link::Stored(t) => Link::Local(Arc::new((**t).clone())),
            Link::Owned(owner, slot) => Link::Owned(owner.clone(), slot.clone()),
            Link::Weak(owner, slot) => {
                Link::Owned(owner.upgrade().expect("live type table"), slot.clone())
            }
        };
        Self {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            link,
        }
    }
}
impl<T> fmt::Debug for Nominal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}
impl<T> PartialEq for Nominal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl<T> Eq for Nominal<T> {}
impl<T> Hash for Nominal<T> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.name.hash(h);
    }
}
impl Types {
    pub fn class(self: &Arc<Self>, name: &str) -> Nominal<ClassType> {
        let slot = Arc::new(OnceLock::new());
        let value = Nominal {
            name: name.into(),
            display_name: None,
            link: Link::Owned(self.clone(), Arc::downgrade(&slot)),
        };
        self.classes.lock().unwrap().push(slot);
        value
    }
    pub fn enumeration(self: &Arc<Self>, name: &str) -> Nominal<EnumType> {
        let slot = Arc::new(OnceLock::new());
        let value = Nominal {
            name: name.into(),
            display_name: None,
            link: Link::Owned(self.clone(), Arc::downgrade(&slot)),
        };
        self.enums.lock().unwrap().push(slot);
        value
    }
}
impl Definition for ClassType {
    fn name(&self) -> &str {
        &self.name
    }
    fn weaken(&mut self) {
        for (_, t) in &mut self.fields {
            *t = t.weaken();
        }
    }
}
impl Definition for EnumType {
    fn name(&self) -> &str {
        &self.name
    }
    fn weaken(&mut self) {
        for v in &mut self.variants {
            for t in &mut v.fields {
                *t = t.weaken();
            }
        }
    }
    fn cached_facts(&self) -> Option<crate::type_facts::Facts> {
        self.facts.get().copied()
    }
    fn cache_facts(&self, facts: crate::type_facts::Facts) {
        let _ = self.facts.set(facts);
    }
}

impl Ty {
    fn weaken(&self) -> Self {
        match self {
            Self::Class(t) => Self::Class(t.weaken()),
            Self::Enum(t) => Self::Enum(t.weaken()),
            Self::List(t) => Self::List(Arc::new(t.weaken())),
            Self::Channel(t, sender) => Self::Channel(Arc::new(t.weaken()), *sender),
            Self::Future(t) => Self::Future(Arc::new(t.weaken())),
            Self::Set(t) => Self::Set(Arc::new(t.weaken())),
            Self::Box(t) => Self::Box(Arc::new(t.weaken())),
            Self::Dict(k, v) => Self::Dict(Arc::new(k.weaken()), Arc::new(v.weaken())),
            Self::Ref(t, m) => Self::Ref(Arc::new(t.weaken()), *m),
            Self::Callable(t) => Self::Callable(Arc::new(crate::op::CallableSig {
                inputs: t.inputs.iter().map(Ty::weaken).collect(),
                output: t.output.as_ref().map(Ty::weaken),
            })),
            Self::BoxedCallable(t) => Self::BoxedCallable(Arc::new(crate::op::CallableSig {
                inputs: t.inputs.iter().map(Ty::weaken).collect(),
                output: t.output.as_ref().map(Ty::weaken),
            })),
            _ => self.clone(),
        }
    }
}

impl Clone for Ty {
    fn clone(&self) -> Self {
        match self {
            Self::Task(t) => Self::Task(t.clone()),
            Self::Channel(t, sender) => Self::Channel(Arc::new((**t).clone()), *sender),
            Self::Future(t) => Self::Future(Arc::new((**t).clone())),
            Self::Class(t) => Self::Class(t.clone()),
            Self::Enum(t) => Self::Enum(t.clone()),
            Self::List(t) => Self::List(Arc::new((**t).clone())),
            Self::Set(t) => Self::Set(Arc::new((**t).clone())),
            Self::Dict(k, v) => Self::Dict(Arc::new((**k).clone()), Arc::new((**v).clone())),
            Self::Box(t) => Self::Box(Arc::new((**t).clone())),
            Self::Ref(t, m) => Self::Ref(Arc::new((**t).clone()), *m),
            Self::Callable(t) => Self::Callable(Arc::new((**t).clone())),
            Self::BoxedCallable(t) => Self::BoxedCallable(t.clone()),
            Self::Closure(t) => Self::Closure(t.clone()),
            Self::Generator(t) => Self::Generator(t.clone()),
            Self::Range(t) => Self::Range(t.clone()),
            Self::ForeignPtr(t) => Self::ForeignPtr(t.clone()),
            Self::I8 => Self::I8,
            Self::I16 => Self::I16,
            Self::I32 => Self::I32,
            Self::I64 => Self::I64,
            Self::U8 => Self::U8,
            Self::U16 => Self::U16,
            Self::U32 => Self::U32,
            Self::U64 => Self::U64,
            Self::F32 => Self::F32,
            Self::F64 => Self::F64,
            Self::Unit => Self::Unit,
            Self::Str => Self::Str,
            Self::File => Self::File,
            Self::Bool => Self::Bool,
            Self::Executor => Self::Executor,
            Self::CancellationToken => Self::CancellationToken,
            Self::FsReadDir => Self::FsReadDir,
            Self::FsDirEntry => Self::FsDirEntry,
            Self::FsMetadata => Self::FsMetadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_fields_can_escape_but_tables_do_not_leak() {
        let owner = Arc::new(Types::default());
        let witness = Arc::downgrade(&owner);
        let node = owner.class("Node");
        node.define(ClassType {
            name: "Node".into(),
            fields: vec![(
                "children".into(),
                Ty::List(Arc::new(crate::sum::option(Ty::Class(node.clone())))),
            )],
            destructor: None,
            fallible_init: false,
            bytes: std::sync::OnceLock::new(),
            managed: std::sync::OnceLock::new(),
        });
        let field = node.get().fields[0].1.clone();
        drop(node);
        drop(owner);
        assert!(witness.upgrade().is_some());
        assert!(field.recursive_data());
        assert_eq!(format!("{field:?}"), "List(Enum(Option[Node]))");
        drop(field);
        assert!(
            witness.upgrade().is_none(),
            "recursive definitions must not own one another"
        );
    }

    #[test]
    fn boxed_recursive_fields_do_not_leak_tables() {
        let owner = Arc::new(Types::default());
        let witness = Arc::downgrade(&owner);
        let node = owner.class("Node");
        node.define(ClassType {
            name: "Node".into(),
            fields: vec![(
                "next".into(),
                crate::sum::option(Ty::Box(Arc::new(Ty::Class(node.clone())))),
            )],
            destructor: None,
            fallible_init: false,
            bytes: std::sync::OnceLock::new(),
            managed: std::sync::OnceLock::new(),
        });
        let field = node.get().fields[0].1.clone();
        drop(node);
        drop(owner);
        assert!(field.recursive_data());
        assert!(!field.facts().infinite);
        drop(field);
        assert!(witness.upgrade().is_none());
    }

    #[test]
    fn partially_resolved_tables_are_freed_after_a_diagnostic() {
        let owner = Arc::new(Types::default());
        let witness = Arc::downgrade(&owner);
        let a = owner.class("A");
        let b = owner.class("B");
        a.define(ClassType {
            name: "A".into(),
            fields: vec![("b".into(), Ty::Class(b.clone()))],
            destructor: None,
            fallible_init: false,
            bytes: std::sync::OnceLock::new(),
            managed: std::sync::OnceLock::new(),
        });
        assert!(!Ty::Class(a.clone()).facts().complete);
        drop(a);
        drop(b);
        drop(owner);
        assert!(witness.upgrade().is_none());
    }
}

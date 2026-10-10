//! Compilation-owned nominal definitions. Stored edges are weak; escaped handles
//! retain the table. Neither recursive declarations nor failed compilation leak
//! an Rc cycle, and looking up a definition never expands its children.
use crate::{op::Ty, record::ClassType, sum::EnumType};
use std::cell::{OnceCell, RefCell};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

type Slot<T> = Rc<OnceCell<Rc<T>>>;

#[derive(Default)]
pub struct Types {
    classes: RefCell<Vec<Slot<ClassType>>>,
    enums: RefCell<Vec<Slot<EnumType>>>,
    facts: RefCell<std::collections::HashMap<String, crate::type_facts::Facts>>,
}

enum Link<T> {
    Local(Rc<T>),
    Stored(Rc<T>),
    Owned(Rc<Types>, Weak<OnceCell<Rc<T>>>),
    Weak(Weak<Types>, Weak<OnceCell<Rc<T>>>),
}

pub struct Nominal<T> {
    pub name: String,
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
            link: Link::Local(Rc::new(value)),
        }
    }
    pub fn get(&self) -> Rc<T> {
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
    pub fn try_get(&self) -> Option<Rc<T>> {
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
        assert!(slot.upgrade().unwrap().set(Rc::new(value)).is_ok());
        if let Some(owner) = self.owner() {
            owner.facts.borrow_mut().clear();
        }
    }
    fn owner(&self) -> Option<Rc<Types>> {
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
        self.owner()?.facts.borrow().get(&self.name).copied()
    }
    pub fn cache_facts(&self, facts: crate::type_facts::Facts) {
        if let Link::Local(t) | Link::Stored(t) = &self.link {
            t.cache_facts(facts);
            return;
        }
        if let Some(owner) = self.owner() {
            owner.facts.borrow_mut().insert(self.name.clone(), facts);
        }
    }
    fn weaken(&self) -> Self {
        let link = match &self.link {
            Link::Owned(owner, slot) => Link::Weak(Rc::downgrade(owner), slot.clone()),
            Link::Weak(owner, slot) => Link::Weak(owner.clone(), slot.clone()),
            Link::Local(value) | Link::Stored(value) => {
                let mut value = (**value).clone();
                value.weaken();
                Link::Stored(Rc::new(value))
            }
        };
        Self {
            name: self.name.clone(),
            link,
        }
    }
}
impl<T: Definition> Clone for Nominal<T> {
    fn clone(&self) -> Self {
        let link = match &self.link {
            Link::Local(t) => Link::Local(t.clone()),
            // Cloning fields out of the table promotes all weak nominal edges.
            Link::Stored(t) => Link::Local(Rc::new((**t).clone())),
            Link::Owned(owner, slot) => Link::Owned(owner.clone(), slot.clone()),
            Link::Weak(owner, slot) => {
                Link::Owned(owner.upgrade().expect("live type table"), slot.clone())
            }
        };
        Self {
            name: self.name.clone(),
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
    pub fn class(self: &Rc<Self>, name: &str) -> Nominal<ClassType> {
        let slot = Rc::new(OnceCell::new());
        let value = Nominal {
            name: name.into(),
            link: Link::Owned(self.clone(), Rc::downgrade(&slot)),
        };
        self.classes.borrow_mut().push(slot);
        value
    }
    pub fn enumeration(self: &Rc<Self>, name: &str) -> Nominal<EnumType> {
        let slot = Rc::new(OnceCell::new());
        let value = Nominal {
            name: name.into(),
            link: Link::Owned(self.clone(), Rc::downgrade(&slot)),
        };
        self.enums.borrow_mut().push(slot);
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
            Self::List(t) => Self::List(Rc::new(t.weaken())),
            Self::Channel(t, sender) => Self::Channel(Rc::new(t.weaken()), *sender),
            Self::Future(t) => Self::Future(Rc::new(t.weaken())),
            Self::Set(t) => Self::Set(Rc::new(t.weaken())),
            Self::Box(t) => Self::Box(Rc::new(t.weaken())),
            Self::Dict(k, v) => Self::Dict(Rc::new(k.weaken()), Rc::new(v.weaken())),
            Self::Ref(t, m) => Self::Ref(Rc::new(t.weaken()), *m),
            Self::Callable(t) => Self::Callable(Rc::new(crate::op::CallableSig {
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
            Self::Channel(t, sender) => Self::Channel(Rc::new((**t).clone()), *sender),
            Self::Future(t) => Self::Future(Rc::new((**t).clone())),
            Self::Class(t) => Self::Class(t.clone()),
            Self::Enum(t) => Self::Enum(t.clone()),
            Self::List(t) => Self::List(Rc::new((**t).clone())),
            Self::Set(t) => Self::Set(Rc::new((**t).clone())),
            Self::Dict(k, v) => Self::Dict(Rc::new((**k).clone()), Rc::new((**v).clone())),
            Self::Box(t) => Self::Box(Rc::new((**t).clone())),
            Self::Ref(t, m) => Self::Ref(Rc::new((**t).clone()), *m),
            Self::Callable(t) => Self::Callable(Rc::new((**t).clone())),
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_fields_can_escape_but_tables_do_not_leak() {
        let owner = Rc::new(Types::default());
        let witness = Rc::downgrade(&owner);
        let node = owner.class("Node");
        node.define(ClassType {
            name: "Node".into(),
            fields: vec![(
                "children".into(),
                Ty::List(Rc::new(crate::sum::option(Ty::Class(node.clone())))),
            )],
            destructor: None,
            fallible_init: false,
            bytes: std::cell::OnceCell::new(),
            managed: std::cell::OnceCell::new(),
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
        let owner = Rc::new(Types::default());
        let witness = Rc::downgrade(&owner);
        let node = owner.class("Node");
        node.define(ClassType {
            name: "Node".into(),
            fields: vec![(
                "next".into(),
                crate::sum::option(Ty::Box(Rc::new(Ty::Class(node.clone())))),
            )],
            destructor: None,
            fallible_init: false,
            bytes: std::cell::OnceCell::new(),
            managed: std::cell::OnceCell::new(),
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
        let owner = Rc::new(Types::default());
        let witness = Rc::downgrade(&owner);
        let a = owner.class("A");
        let b = owner.class("B");
        a.define(ClassType {
            name: "A".into(),
            fields: vec![("b".into(), Ty::Class(b.clone()))],
            destructor: None,
            fallible_init: false,
            bytes: std::cell::OnceCell::new(),
            managed: std::cell::OnceCell::new(),
        });
        assert!(!Ty::Class(a.clone()).facts().complete);
        drop(a);
        drop(b);
        drop(owner);
        assert!(witness.upgrade().is_none());
    }
}

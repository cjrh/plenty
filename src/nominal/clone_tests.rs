use super::*;

fn definition(name: &str, fields: Vec<(String, Ty)>) -> ClassType {
    ClassType {
        name: name.into(),
        fields,
        destructor: None,
        fallible_init: false,
        bytes: OnceLock::new(),
        managed: OnceLock::new(),
    }
}

#[test]
fn owned_structural_subtrees_share_storage() {
    let owner = Arc::new(Types::default());
    let node = owner.class("Node");
    node.define(definition("Node", vec![]));
    for inner in [Ty::I64, Ty::Class(node), crate::sum::option(Ty::Str)] {
        let leaf = Arc::new(inner);
        let nested = Arc::new(Ty::List(leaf));
        let ty = Ty::Dict(Arc::new(Ty::Str), nested);
        let Ty::Dict(key, value) = &ty else {
            unreachable!()
        };
        let Ty::Dict(cloned_key, cloned_value) = ty.clone() else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(key, &cloned_key));
        assert!(Arc::ptr_eq(value, &cloned_value));
    }
}

#[test]
fn cloning_stored_fields_promotes_all_structural_paths() {
    let wrappers: &[fn(Ty) -> Ty] = &[
        |t| Ty::List(Arc::new(t)),
        |t| Ty::Set(Arc::new(t)),
        |t| Ty::Box(Arc::new(t)),
        |t| Ty::Shared(Arc::new(t)),
        |t| Ty::Future(Arc::new(t)),
        |t| Ty::Channel(Arc::new(t), true),
        |t| Ty::Channel(Arc::new(t), false),
        |t| Ty::Ref(Arc::new(t), true),
        |t| Ty::Ref(Arc::new(t), false),
        |t| Ty::Dict(Arc::new(t), Arc::new(Ty::I64)),
        |t| Ty::Dict(Arc::new(Ty::Str), Arc::new(t)),
        |t| {
            Ty::Callable(Arc::new(crate::op::CallableSig {
                inputs: vec![t],
                output: None,
            }))
        },
        |t| {
            Ty::Callable(Arc::new(crate::op::CallableSig {
                inputs: vec![],
                output: Some(t),
            }))
        },
        |t| {
            Ty::Callable(Arc::new(crate::op::CallableSig {
                inputs: vec![Ty::I64, t, Ty::Str],
                output: Some(Ty::Bool),
            }))
        },
        |t| {
            Ty::BoxedCallable(Arc::new(crate::op::CallableSig {
                inputs: vec![t],
                output: None,
            }))
        },
        |t| {
            Ty::BoxedCallable(Arc::new(crate::op::CallableSig {
                inputs: vec![],
                output: Some(t),
            }))
        },
    ];
    for wrap in wrappers {
        let owner = Arc::new(Types::default());
        let witness = Arc::downgrade(&owner);
        let node = owner.class("Node");
        let nested = Ty::List(Arc::new(wrap(Ty::Class(node.clone()))));
        node.define(definition("Node", vec![("next".into(), nested)]));
        let escaped = node.get().fields[0].1.clone();
        drop(node);
        drop(owner);
        assert!(
            witness.upgrade().is_some(),
            "escaped field must own its table"
        );
        let second = escaped.clone();
        assert_eq!(escaped, second);
        drop(escaped);
        assert!(witness.upgrade().is_some());
        drop(second);
        assert!(
            witness.upgrade().is_none(),
            "stored fields must not retain the table"
        );
    }
}

#[test]
fn owned_callable_signatures_share_storage() {
    let owner = Arc::new(Types::default());
    let node = owner.class("Node");
    node.define(definition("Node", vec![]));
    let sig = Arc::new(crate::op::CallableSig {
        inputs: vec![Ty::Class(node.clone()), Ty::List(Arc::new(Ty::Str))],
        output: Some(Ty::Class(node)),
    });
    for ty in [Ty::Callable(sig.clone()), Ty::BoxedCallable(sig.clone())] {
        let (Ty::Callable(cloned) | Ty::BoxedCallable(cloned)) = ty.clone() else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(&sig, &cloned));
    }
}

#[test]
fn promotion_rebuilds_only_the_weak_branch() {
    let owner = Arc::new(Types::default());
    let node = owner.class("Node");
    node.define(definition("Node", vec![]));
    let key = Arc::new(Ty::List(Arc::new(Ty::Str)));
    let weak = Arc::new(Ty::Class(node.weaken()));
    let stored = Ty::Dict(key.clone(), weak.clone());
    let Ty::Dict(cloned_key, promoted) = stored.clone() else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(&key, &cloned_key));
    assert!(!Arc::ptr_eq(&weak, &promoted));
    let Ty::Class(promoted) = promoted.as_ref() else {
        unreachable!()
    };
    assert!(matches!(promoted.link, Link::Owned(..)));
}

#[test]
fn unresolved_enum_edges_are_promoted_without_expanding_definitions() {
    let owner = Arc::new(Types::default());
    let witness = Arc::downgrade(&owner);
    let enumeration = owner.enumeration("Later");
    let field = Ty::List(Arc::new(Ty::Enum(enumeration.weaken())));
    let escaped = field.clone();
    drop(field);
    drop(enumeration);
    drop(owner);
    assert!(witness.upgrade().is_some());
    let Ty::List(inner) = &escaped else {
        unreachable!()
    };
    let Ty::Enum(enumeration) = inner.as_ref() else {
        unreachable!()
    };
    assert!(enumeration.try_get().is_none());
    drop(escaped);
    assert!(witness.upgrade().is_none());
}

#[test]
fn nested_stored_structural_sums_escape_without_cycles() {
    let owner = Arc::new(Types::default());
    let witness = Arc::downgrade(&owner);
    let node = owner.class("Node");
    let field = Ty::List(Arc::new(crate::sum::option(Ty::Box(Arc::new(Ty::Class(
        node.clone(),
    ))))));
    node.define(definition("Node", vec![("next".into(), field)]));
    let escaped = node.get().fields[0].1.clone();
    drop(node);
    drop(owner);
    assert!(escaped.recursive_data());
    assert!(witness.upgrade().is_some());
    drop(escaped);
    assert!(witness.upgrade().is_none());
}

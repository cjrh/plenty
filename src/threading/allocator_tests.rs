//! Compiler admission tests inject the private marker into typed IR. There is
//! deliberately no public frontend provider constructor or ABI adapter yet.
use super::*;
use crate::op::{CallableSig, FnSig};

fn function(name: &str, input: Option<Ty>, body: Vec<Op>) -> Op {
    Op::DefineFn(
        name.into(),
        Box::new(CompiledFn {
            location: None,
            origins: Default::default(),
            generator: None,
            sig: Arc::new(FnSig {
                inputs: input.into_iter().map(|ty| ("state".into(), ty)).collect(),
                outputs: vec![],
            }),
            doc: "".into(),
            body: body.into(),
            locals: Arc::new([]),
        }),
    )
}
fn marker(state: Ty) -> Op {
    Op::Thread(ThreadOp::AllocatorProvider(Arc::new(AllocatorProvider {
        state,
        allocate: "allocate".into(),
        release: "release".into(),
    })))
}
fn program(state: Ty, allocation: Vec<Op>, release: Vec<Op>) -> Vec<Op> {
    vec![
        function(
            "allocate",
            Some(Ty::Ref(Arc::new(state.clone()), true)),
            allocation,
        ),
        function(
            "release",
            Some(Ty::Ref(Arc::new(state.clone()), true)),
            release,
        ),
        marker(state),
    ]
}
fn foreign(thread_safe: bool, host_callback: bool) -> Op {
    Op::ForeignCall {
        declaration: crate::foreign::Declaration {
            target: crate::foreign::Target::Symbol("provider_effect".into()),
            arguments: vec![],
            thread_safe,
            host_callback,
        },
        sig: Arc::new(FnSig {
            inputs: vec![],
            outputs: vec![],
        }),
    }
}
fn owner(field: Ty, destructor: bool) -> Ty {
    Ty::Class(crate::nominal::Nominal::new(crate::record::ClassType {
        name: "Policy".into(),
        fields: vec![("field".into(), field)],
        destructor: destructor.then(|| "destroy".into()),
        fallible_init: false,
        copy: false,
        forwards_operators: false,
        redacted: false,
        quantity: None,
        bounds: None,
        bytes: Default::default(),
        managed: Default::default(),
    }))
}
fn denied(ops: &[Op], text: &str) {
    let error = check(ops).unwrap_err().to_string();
    assert!(error.contains("allocator provider"), "{error}");
    assert!(error.contains(text), "{error}");
}

#[test]
fn provider_root_checks_effects_without_a_worker_spawn() {
    check(&program(Ty::I64, vec![foreign(true, false)], vec![])).unwrap();
    denied(
        &program(Ty::I64, vec![foreign(false, false)], vec![]),
        "no worker-thread effect contract",
    );
    denied(
        &program(Ty::I64, vec![], vec![foreign(false, false)]),
        "release",
    );
    denied(
        &program(Ty::I64, vec![foreign(true, true)], vec![]),
        "foreign host entry context",
    );
}

#[test]
fn callbacks_are_required_and_mutably_borrow_exact_state() {
    let mut ops = program(Ty::I64, vec![], vec![]);
    ops.remove(1);
    denied(&ops, "unavailable callback `release`");
    let mut ops = program(Ty::I64, vec![], vec![]);
    for bad in [
        None,
        Some(Ty::I64),
        Some(Ty::Ref(Arc::new(Ty::I64), false)),
        Some(Ty::Ref(Arc::new(Ty::U64), true)),
    ] {
        ops[0] = function("allocate", bad, vec![]);
        denied(&ops, "must take `&mut i64`");
    }
}

#[test]
fn state_rejects_borrowed_hidden_channel_and_uncertified_pointer_fields() {
    for state in [
        Ty::Ref(Arc::new(Ty::I64), false),
        owner(Ty::Channel(Arc::new(Ty::I64), true), false),
    ] {
        denied(&program(state, vec![], vec![]), "cannot retain");
    }
    denied(
        &program(
            owner(Ty::ForeignPtr("Handle".into(), Threads::None), false),
            vec![],
            vec![],
        ),
        "no thread contract",
    );
}

#[test]
fn movable_state_requires_an_affine_owner_but_not_sync() {
    let state = owner(Ty::ForeignPtr("Handle".into(), Threads::Movable), true);
    let mut ops = program(state.clone(), vec![], vec![]);
    ops.push(function(
        "destroy",
        Some(Ty::Ref(Arc::new(state), true)),
        vec![],
    ));
    check(&ops).unwrap();
    denied(
        &program(
            owner(Ty::ForeignPtr("Handle".into(), Threads::Movable), false),
            vec![],
            vec![],
        ),
        "not `@thread_shareable`",
    );
}

#[test]
fn state_destructors_and_transitive_callback_helpers_are_checked() {
    let state = owner(Ty::I64, true);
    let mut ops = program(state.clone(), vec![], vec![]);
    ops.push(function(
        "destroy",
        Some(Ty::Ref(Arc::new(state), true)),
        vec![Op::Call("helper".into())],
    ));
    ops.push(function("helper", None, vec![foreign(false, false)]));
    denied(&ops, "helper");
    let mut ops = program(Ty::I64, vec![Op::Call("helper".into())], vec![]);
    ops.push(function("helper", None, vec![foreign(false, false)]));
    denied(&ops, "helper");
}

#[test]
fn indirect_targets_cannot_hide_unsafe_effects() {
    let signature = Arc::new(CallableSig {
        inputs: vec![],
        output: None,
    });
    let mut ops = program(Ty::I64, vec![Op::CallIndirect(signature.clone())], vec![]);
    denied(&ops, "no statically known targets");
    ops.extend([
        function("safe", None, vec![]),
        Op::FunctionAddress("safe".into(), signature.clone()),
    ]);
    check(&ops).unwrap();
    ops.extend([
        function("unsafe", None, vec![foreign(false, false)]),
        Op::FunctionAddress("unsafe".into(), signature),
    ]);
    denied(&ops, "provider_effect");
}

#[test]
fn actor_restriction_is_not_bypassed_by_worker_check_cache() {
    let mut ops = program(Ty::I64, vec![Op::Thread(ThreadOp::Forget(Ty::I64))], vec![]);
    // The ordinary worker traversal currently admits this raw IR operation.
    // Provider admission must still run its stricter check on the same body.
    ops.insert(
        0,
        Op::Thread(ThreadOp::Start(
            Arc::new(Task {
                adapter: "unused".into(),
                worker: "allocate".into(),
                closure: None,
                inputs: vec![Ty::I64],
                output: Ty::Unit,
            }),
            0,
            false,
        )),
    );
    denied(&ops, "actor execution and abandonment");
}

#[test]
fn fixed_array_elements_are_checked_for_existing_workers_and_providers() {
    for count in [0, 1, 4] {
        let state = Ty::Array(
            Arc::new(Ty::ForeignPtr("Handle".into(), Threads::None)),
            count,
        );
        assert!(check(&[Op::Thread(ThreadOp::Rows(state.clone(), false))]).is_err());
        denied(&program(state, vec![], vec![]), "no thread contract");
        let hidden = Ty::Array(
            Arc::new(Ty::Array(Arc::new(Ty::Channel(Arc::new(Ty::I64), true)), 1)),
            count,
        );
        assert!(crate::boxed::erasure_barrier(&hidden));
        denied(&program(hidden, vec![], vec![]), "cannot retain");
    }
    let state = Ty::Array(Arc::new(owner(Ty::I64, true)), 2);
    let ops = vec![
        Op::Thread(ThreadOp::Rows(state, false)),
        function("destroy", None, vec![foreign(false, false)]),
    ];
    assert!(
        check(&ops)
            .unwrap_err()
            .to_string()
            .contains("provider_effect")
    );
}

#[test]
fn marker_keeps_both_callback_bodies_reachable() {
    let mut names = Vec::new();
    function_references(&marker(Ty::I64), &mut |name| names.push(name.to_owned()));
    assert_eq!(names, ["allocate", "release"]);
    crate::op::check(&program(Ty::I64, vec![], vec![])).unwrap();
}

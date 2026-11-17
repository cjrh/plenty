//! Focused admission tests inject the private marker directly into typed IR.
//! Compiled frontend/ABI coverage lives in test_std_alloc_provider.rs.
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
fn provider_and_worker_roots_still_obey_program_wide_actor_exclusion() {
    let mut ops = program(Ty::I64, vec![Op::Thread(ThreadOp::Forget(Ty::I64))], vec![]);
    // Ordinary worker certification alone admits this raw IR operation.
    // Whole-program exclusion must reject the combination before that traversal.
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
    assert!(
        check(&ops)
            .unwrap_err()
            .to_string()
            .contains("same program")
    );
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

fn actor_heap_body() -> Vec<Op> {
    vec![
        Op::PushInt(crate::value::Value::I64(0)),
        Op::PushInt(crate::value::Value::I64(4096)),
        Op::Collection(crate::collection::CollectionOp::ActorHeap),
        Op::Drop,
    ]
}

fn context_denied(ops: &[Op]) {
    let message = check(ops).unwrap_err().to_string();
    assert!(message.contains("retained allocator use"), "{message}");
    assert!(message.contains("same program"), "{message}");
}

#[test]
fn system_only_use_excludes_actors_without_any_certification_roots() {
    let task = Arc::new(Task {
        adapter: "unused".into(),
        worker: "actor".into(),
        closure: None,
        inputs: vec![],
        output: Ty::Unit,
    });
    for operation in [
        Op::Thread(ThreadOp::Run(task)),
        Op::Thread(ThreadOp::Forget(Ty::I64)),
        Op::Collection(crate::collection::CollectionOp::ActorHeap),
    ] {
        let mut ops = vec![operation, Op::Thread(ThreadOp::AllocatorUse)];
        context_denied(&ops);
        ops.reverse();
        context_denied(&ops);
    }
}

#[test]
fn unrelated_and_unused_bodies_are_conservatively_excluded() {
    let mut ops = vec![
        function("unused_actor", None, actor_heap_body()),
        function(
            "independent_allocator",
            None,
            vec![Op::Thread(ThreadOp::AllocatorUse)],
        ),
    ];
    // The independent IR checker reaches the same gate with well-typed bodies.
    assert!(
        crate::op::check(&ops)
            .unwrap_err()
            .to_string()
            .contains("same program")
    );
    context_denied(&ops);
    ops.reverse();
    context_denied(&ops);
    // Custom registration implies use; it cannot avoid the gate by omitting
    // the system/use-only marker.
    let mut ops = program(Ty::I64, vec![], vec![]);
    ops.push(function("unused_actor", None, actor_heap_body()));
    context_denied(&ops);
}

#[test]
fn nested_bodies_cannot_hide_either_half_of_the_exclusion() {
    let actor = function("unused_actor", None, actor_heap_body());
    let retained = Op::Thread(ThreadOp::AllocatorUse);
    let nested = |body: Vec<Op>| Op::Loop {
        condition: Arc::new([]),
        body: body.into(),
    };
    for ops in [
        vec![actor.clone(), nested(vec![retained.clone()])],
        vec![retained, nested(vec![actor])],
    ] {
        context_denied(&ops);
    }
}

#[test]
fn either_feature_alone_keeps_its_existing_checks() {
    crate::op::check(&[function("actor", None, actor_heap_body())]).unwrap();
    crate::op::check(&[Op::Thread(ThreadOp::AllocatorUse)]).unwrap();
    let mut ops = program(Ty::I64, vec![], vec![]);
    ops.push(Op::Thread(ThreadOp::AllocatorUse));
    crate::op::check(&ops).unwrap();
    let mut names = Vec::new();
    function_references(&Op::Thread(ThreadOp::AllocatorUse), &mut |name| {
        names.push(name.to_owned())
    });
    assert!(names.is_empty());
}

#[test]
fn thread_safe_exports_do_not_relax_program_wide_exclusion() {
    let mut exports = [crate::exports::Export {
        function: "entry".into(),
        name: "entry".into(),
        symbol: "entry".into(),
        signature: Arc::new(FnSig {
            inputs: vec![],
            outputs: vec![],
        }),
        documentation: String::new(),
        thread_safe: true,
        host_context: false,
        exact: false,
    }];
    let ops = vec![
        function("entry", None, vec![Op::Thread(ThreadOp::AllocatorUse)]),
        function("unused_actor", None, actor_heap_body()),
    ];
    assert!(
        check_exports(&ops, &mut exports)
            .unwrap_err()
            .to_string()
            .contains("same program")
    );
}

#[test]
fn marker_keeps_both_callback_bodies_reachable() {
    let mut names = Vec::new();
    function_references(&marker(Ty::I64), &mut |name| names.push(name.to_owned()));
    assert_eq!(names, ["allocate", "release"]);
    crate::op::check(&program(Ty::I64, vec![], vec![])).unwrap();
}

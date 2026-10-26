//! Omit unused standard-library functions from native emission.
//!
//! Checking has already visited every body. Generators remain roots: their
//! implicit type-driven references do not have to be reconstructed here. User
//! definitions remain roots too, preserving the existing object/debug
//! contract. This is deliberately narrower than general dead-code elimination.
//!
//! A class's `__del__` is live when a live body arms it (`ClassOp::ArmDrop`,
//! issue #506). Release runs the hook only for an instance whose state word
//! was armed, and only `ArmDrop` arms one, so this covers every path that can
//! destroy an instance: fields, containers, payloads, captures, messages, and
//! jobs all hold values some live body constructed. A descriptor of an
//! unarmed class can still be emitted; it then has no hook.
use super::*;
use crate::hash::HashSet;

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn prune(ops: &mut Vec<Op>, exports: &[crate::exports::Export]) {
    let candidates: HashMap<&str, &crate::op::CompiledFn> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, f) if f.origins.standard_library && f.generator.is_none() => {
                Some((name.as_str(), f.as_ref()))
            }
            _ => None,
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let mut pending = Vec::new();
    let mut live = HashSet::default();
    let mut require = |name: &str| {
        if let Some((&name, _)) = candidates.get_key_value(name)
            && live.insert(name)
        {
            pending.push(name);
        }
    };
    for export in exports {
        require(&export.function);
    }
    for op in ops.iter() {
        if matches!(op, Op::DefineFn(name, _) if candidates.contains_key(name.as_str())) {
            continue;
        }
        references(std::slice::from_ref(op), &mut require);
    }
    while let Some(name) = pending.pop() {
        references(&candidates[name].body, &mut |name| {
            if let Some((&name, _)) = candidates.get_key_value(name)
                && live.insert(name)
            {
                pending.push(name);
            }
        });
    }
    // Keep input order. Declaration order controls object symbol numbering.
    let dead: HashSet<String> = candidates
        .keys()
        .filter(|name| !live.contains(**name))
        .map(|name| (*name).to_owned())
        .collect();
    ops.retain(|op| !matches!(op, Op::DefineFn(name, _) if dead.contains(name)));
}

fn references(ops: &[Op], require: &mut impl FnMut(&str)) {
    // Type-driven calls target generators, which remain roots.
    crate::threading::walk(ops, &mut |op| {
        crate::threading::function_references(op, require);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{CallableSig, CompiledFn, FnSig};

    fn definition(name: &str, standard: bool, body: Vec<Op>) -> Op {
        Op::DefineFn(
            name.into(),
            Box::new(CompiledFn {
                location: None,
                origins: Arc::new(crate::ownership::Origins {
                    standard_library: standard,
                    ..Default::default()
                }),
                generator: None,
                sig: Arc::new(FnSig {
                    inputs: vec![],
                    outputs: vec![],
                }),
                doc: "".into(),
                body: body.into(),
                locals: Arc::default(),
            }),
        )
    }

    fn names(ops: &[Op]) -> Vec<&str> {
        ops.iter()
            .filter_map(|op| match op {
                Op::DefineFn(name, _) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn removes_unreachable_cycles_and_keeps_transitive_calls_in_source_order() {
        let mut ops = vec![
            definition("std.live", true, vec![Op::TailCall("std.child".into())]),
            definition("std.dead", true, vec![Op::Call("std.other".into())]),
            definition("std.other", true, vec![Op::Call("std.dead".into())]),
            definition("std.child", true, vec![Op::Call("std.live".into())]),
            definition("main", false, vec![Op::Call("std.live".into())]),
        ];
        prune(&mut ops, &[]);
        assert_eq!(names(&ops), ["std.live", "std.child", "main"]);
    }

    #[test]
    fn preserves_user_bodies_implicit_roots_and_exports() {
        let mut generator = definition("std.generator", true, vec![Op::Call("std.helper".into())]);
        let Op::DefineFn(_, f) = &mut generator else {
            unreachable!()
        };
        f.generator = Some(Ty::I64);
        let mut ops = vec![
            definition("std.user_name", false, vec![]),
            generator,
            definition("std.helper", true, vec![]),
            definition("std.export", true, vec![]),
            definition("std.dead", true, vec![]),
        ];
        let exports = [crate::exports::Export {
            function: "std.export".into(),
            name: "export".into(),
            symbol: "export".into(),
            signature: Arc::new(FnSig {
                inputs: vec![],
                outputs: vec![],
            }),
            documentation: String::new(),
        }];
        prune(&mut ops, &exports);
        assert_eq!(
            names(&ops),
            ["std.user_name", "std.generator", "std.helper", "std.export"]
        );
    }

    fn armed(name: &str) -> Op {
        Op::Class(crate::record::ClassOp::ArmDrop(
            crate::nominal::Nominal::new(crate::record::ClassType {
                name: name.into(),
                fields: vec![],
                destructor: Some(crate::record::method(name, "__del__")),
                fallible_init: false,
                bytes: Default::default(),
                managed: Default::default(),
            }),
        ))
    }

    #[test]
    fn keeps_a_standard_destructor_only_when_a_live_body_arms_its_class() {
        let destructor = |class: &str, helper: &str| {
            definition(
                &crate::record::method(class, "__del__"),
                true,
                vec![Op::Call(helper.into())],
            )
        };
        let mut ops = vec![
            destructor("std.Armed", "std.armed_helper"),
            destructor("std.Unused", "std.unused_helper"),
            destructor("std.Nested", "std.nested_helper"),
            // Arming happens in a std constructor reached only from `main`,
            // inside control flow; the armed destructor arms another class.
            definition(
                "std.make",
                true,
                vec![Op::If {
                    then: vec![armed("std.Armed")].into(),
                    elifs: Arc::default(),
                    otherwise: Arc::default(),
                }],
            ),
            definition("std.unused_make", true, vec![armed("std.Unused")]),
            definition("std.armed_helper", true, vec![armed("std.Nested")]),
            definition("std.unused_helper", true, vec![]),
            definition("std.nested_helper", true, vec![]),
            definition("main", false, vec![Op::Call("std.make".into())]),
        ];
        prune(&mut ops, &[]);
        assert_eq!(
            names(&ops),
            [
                "__plenty_class_std.Armed.__del__",
                "__plenty_class_std.Nested.__del__",
                "std.make",
                "std.armed_helper",
                "std.nested_helper",
                "main"
            ]
        );
    }

    #[test]
    fn follows_indirect_callbacks_and_adapter_calls_inside_control_flow() {
        let signature = Arc::new(CallableSig {
            inputs: vec![],
            output: None,
        });
        let job = Arc::new(crate::executor::Job {
            adapter: "executor_adapter".into(),
            worker: "std.job".into(),
            input: Ty::I64,
            output: Ty::I64,
            closure: None,
            shared: None,
        });
        let task = Arc::new(crate::threading::Task {
            adapter: "thread_adapter".into(),
            worker: "std.worker".into(),
            inputs: vec![],
            output: Ty::Unit,
            closure: None,
        });
        let callbacks = vec![
            Op::FunctionAddress("std.address".into(), signature.clone()),
            Op::BoxedCallableNew(Box::new(Ty::Unit), "std.boxed".into(), signature),
            Op::Executor(crate::executor::ExecutorOp::Submit(job, false)),
            Op::Thread(crate::threading::ThreadOp::Start(task, 0)),
        ];
        let mut ops = vec![definition(
            "main",
            false,
            vec![Op::Loop {
                condition: Arc::default(),
                body: callbacks.into(),
            }],
        )];
        for name in [
            "std.address",
            "std.boxed",
            "std.job",
            "std.worker",
            "std.dead",
        ] {
            ops.push(definition(name, true, vec![]));
        }
        prune(&mut ops, &[]);
        assert_eq!(
            names(&ops),
            ["main", "std.address", "std.boxed", "std.job", "std.worker"]
        );
    }

    #[test]
    fn real_standard_library_remains_valid_and_deterministic_after_pruning() {
        let mut heap = Heap::default();
        let program = crate::frontend::compile(
            "import std.http.server\n\ndef main() -> Result[(), IoError]:\n    print(42)?\n    Ok(())\n",
            &mut heap,
        ).unwrap();
        crate::op::check(&program.ops).unwrap();
        let before = names(&program.ops).len();
        let mut ops = program.ops;
        prune(&mut ops, &program.exports);
        assert!(names(&ops).len() < before);
        crate::op::check(&ops).unwrap();
        let serial = super::super::object_bytes(
            ops.clone(),
            &heap,
            Some(true),
            &[],
            None,
            1,
            Build::default(),
        )
        .unwrap();
        let parallel =
            super::super::object_bytes(ops, &heap, Some(true), &[], None, 8, Build::default())
                .unwrap();
        assert_eq!(serial, parallel);
    }
}

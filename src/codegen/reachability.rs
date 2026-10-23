//! Omit unused standard-library functions from native emission.
//!
//! Checking has already visited every body. Destructors and generators remain
//! roots: their implicit type-driven references do not have to be reconstructed
//! here. User definitions remain roots too, preserving the existing
//! object/debug contract. This is deliberately narrower than general
//! dead-code elimination.
use super::*;
use crate::hash::HashSet;

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(super) fn prune(ops: &mut Vec<Op>, exports: &[crate::exports::Export]) {
    let candidates: HashMap<&str, &crate::op::CompiledFn> = ops
        .iter()
        .filter_map(|op| match op {
            Op::DefineFn(name, f)
                if f.origins.standard_library
                    && !name.ends_with(".__del__")
                    && f.generator.is_none() =>
            {
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
    crate::threading::walk(ops, &mut |op| match op {
        Op::Call(name)
        | Op::TailCall(name)
        | Op::FunctionAddress(name, _)
        | Op::BoxedCallableNew(_, name, _) => require(name),
        Op::ClosureNew(t) | Op::ClosureCall(t) => require(&t.name),
        Op::Thread(crate::threading::ThreadOp::Start(task, _)) => {
            require(&task.worker);
            if let Some(closure) = &task.closure {
                require(&closure.name);
            }
        }
        Op::Executor(operation) => {
            if let Some(job) = operation.job() {
                require(&job.worker);
                for closure in job.closure.iter().chain(job.shared.iter()) {
                    require(&closure.name);
                }
            }
        }
        Op::Collection(crate::collection::CollectionOp::ListSortBy(sort)) => {
            require(&sort.key_of);
        }
        // Type-driven calls target destructors or generators, which are
        // retained as roots regardless of these explicit references.
        _ => {}
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
            definition(
                "__plenty_class_std.Owner.__del__",
                true,
                vec![Op::Call("std.helper".into())],
            ),
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
            [
                "std.user_name",
                "__plenty_class_std.Owner.__del__",
                "std.generator",
                "std.helper",
                "std.export"
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

//! Lexically scoped native jobs reuse the existing context cleanup obligations.
use super::*;
use crate::threading::{Task, ThreadOp};

/// The start call of `with spawn(...)? as task:` or `.unwrap()`, whether it
/// propagates, and the arguments of a `drop_job()` that makes its error
/// job-free.
pub(super) fn spawn_expression(manager: &Expr) -> Option<(&Expr, bool, Option<&[Expr]>)> {
    let (call, propagate) = match &ungroup(manager).kind {
        Expression::Try(call) => (&**call, true),
        Expression::Method(call, name, args, _) if name == "unwrap" && args.is_empty() => {
            (&**call, false)
        }
        _ => return None,
    };
    let (call, drop_job) = match &ungroup(call).kind {
        Expression::Method(start, name, args, _) if name == "drop_job" => {
            (&**start, Some(&args[..]))
        }
        _ => (call, None),
    };
    matches!(&ungroup(call).kind, Expression::Call(name, _) if name == "spawn").then_some((
        ungroup(call),
        propagate,
        drop_job,
    ))
}

const DROP_JOB_RECEIVER: &str =
    "drop_job requires a SpawnError or SubmitError, or a Result whose error is one";

impl Lower<'_> {
    pub(super) fn with_thread(
        &mut self,
        call: &Expr,
        propagate: bool,
        drop_job: Option<&[Expr]>,
        name: Option<&str>,
        body: &[Stmt],
        ops: &mut Vec<Op>,
    ) -> Result<BlockResult> {
        let Expression::Call(_, args) = &call.kind else {
            unreachable!()
        };
        let Some((worker, args)) = args.split_first() else {
            return Err(call
                .at
                .error("spawn requires a named function or an explicitly borrowed closure"));
        };
        let saved = self.names.clone();
        let start = self.locals.len();
        let manager_start = ops.len();
        let (worker_name, closure, inputs, output, loans) = match &ungroup(worker).kind {
            Expression::Name(function) if !self.names.contains_key(function) => {
                let begin = ops.len();
                let output = self
                    .call_named(function, args, &worker.at, ops)?
                    .unwrap_or(Ty::Unit);
                let (index, symbol) = take_call(ops, begin).ok_or_else(|| {
                    call.at
                        .error("spawn requires a statically selected function")
                })?;
                let inputs: Vec<_> = self.sigs[&symbol]
                    .inputs
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect();
                let loans: Vec<_> = ops[index..]
                    .iter()
                    .filter_map(|op| match op {
                        Op::UseLoan(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                for &loan in &loans {
                    self.check_stored_reference(loan, &call.at)?;
                }
                (symbol, None, inputs, output, loans)
            }
            Expression::Unary(op, value) if op == "&" || op == "&mut" => {
                if !args.is_empty() {
                    return Err(call
                        .at
                        .error("a borrowed closure job takes no additional arguments"));
                }
                let Some(Ty::Closure(closure)) = self.place_type(value) else {
                    return Err(worker
                        .at
                        .error("spawn requires a borrowed reusable closure"));
                };
                if closure.once || !closure.signature.inputs.is_empty() {
                    return Err(worker.at.error("spawn requires a reusable zero-argument closure; capture task inputs explicitly"));
                }
                if closure.mutable && op != "&mut" {
                    return Err(worker
                        .at
                        .error("this worker closure requires &mut borrowing"));
                }
                let (ty, loan) = self.borrow(value, op == "&mut", ops)?;
                let output = closure.signature.output.clone().unwrap_or(Ty::Unit);
                (
                    closure.name.clone(),
                    Some(closure),
                    vec![ty],
                    output,
                    vec![loan],
                )
            }
            _ => {
                if !args.is_empty() {
                    return Err(call.at.error("an owned closure job takes no additional arguments; capture inputs explicitly"));
                }
                let ty = self.value(worker, ops)?;
                let Ty::Closure(closure) = &ty else {
                    return Err(worker
                        .at
                        .error("spawn requires a named function or a concrete closure"));
                };
                if !ty.heap_storable() || !closure.signature.inputs.is_empty() {
                    return Err(worker.at.error("an owned worker must be a concrete zero-argument closure with wholly owned captures and no generator frames"));
                }
                (
                    closure.name.clone(),
                    Some(closure.clone()),
                    vec![ty.clone()],
                    closure.signature.output.clone().unwrap_or(Ty::Unit),
                    vec![],
                )
            }
        };
        if !output.heap_storable() {
            return Err(call.at.error(
                "worker results cannot contain references, generators, borrowed/unresolved closures, or scoped tasks",
            ));
        }
        let task = Arc::new(Task {
            adapter: format!(
                "__plenty_thread_{}_{}",
                self.function_name,
                self.locals.len()
            ),
            worker: worker_name,
            closure,
            inputs,
            output,
        });
        if task.bytes() > i32::MAX as usize {
            return Err(call
                .at
                .error("thread task exceeds native stack-layout limit"));
        }
        let slot = self.slot(Ty::Task(task.clone()), &call.at)?;
        // The start, and a failed start's exit, belong to the call, not its last argument.
        self.mark(&call.at, ops);
        self.holds_tokens(manager_start, ops);
        ops.push(Op::Thread(ThreadOp::Start(task.clone(), slot)));
        let mut source = task.start_result();
        if let Some(args) = drop_job {
            if !args.is_empty() {
                return Err(call.at.error("drop_job takes no arguments"));
            }
            source = self.drop_job(source, &call.at, ops)?;
        }
        let Ty::Enum(source) = source else {
            unreachable!()
        };
        if propagate {
            let Some(Some(Ty::Enum(target))) = self.return_type.clone() else {
                return Err(call.at.error("`?` requires a function returning Result"));
            };
            // The same rule as ordinary `?`: the start error may widen into a sum.
            let target = self.infer_propagated_producers(target, &source);
            if !target.accepts_result_error(&source) {
                let error = &source.get().variants[1].fields[0];
                let declared = &target.get().variants[1].fields[0];
                let hint = super::producer_conflict_hint(declared, error);
                return Err(call.at.error(format!(
                    "spawn propagation requires a Result whose error accepts {error}; add it to the declared error, such as Result[_, {declared} | {error}]{hint}"
                )));
            }
            let mut cleanup = Vec::new();
            self.cleanup(0, &mut cleanup);
            ops.push(Op::Try {
                source: Box::new(source),
                target: Box::new(target),
                cleanup: cleanup.into(),
            });
        } else {
            ops.push(Op::Enum(crate::sum::EnumOp::Unwrap(source)));
        }
        ops.push(Op::Drop);
        self.building(&call.at, manager_start, ops, false);
        let mut exit = vec![Op::Thread(ThreadOp::Finish(task.clone(), slot))];
        exit.extend(loans.into_iter().map(Op::UseLoan));
        self.contexts.push(contexts::Context {
            slot,
            exit,
            loan: None,
            kind: contexts::Kind::Thread,
            at: site(&call.at, true),
        });
        if let Some(name) = name {
            if self.names.contains_key(name) {
                return Err(call.at.error(format!("duplicate binding `{name}`")));
            }
            self.bind(
                name.into(),
                Local {
                    slot,
                    ty: Ty::Task(task),
                    mutable: false,
                },
            );
        }
        let result = self.block_inner(body, ops, &Tail::Discard)?;
        if matches!(result, BlockResult::Continues(_)) {
            self.cleanup(start, ops);
        }
        self.contexts.pop();
        self.names = saved;
        Ok(result)
    }

    /// `drop_job()` on a `SpawnError[F]` or `SubmitError[F]`, or on a Result
    /// carrying one, as a method call on an expression.
    pub(super) fn drop_job_method(
        &mut self,
        base: &Expr,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let actual = self.expr_expected(base, None, ops)?;
        if let Some(actual) = &actual {
            let receiver = classes::unbox(actual.clone());
            if classes::inherent_receiver(&receiver) && crate::sum::job_free(&receiver).is_none() {
                self.unbox_to(actual.clone(), &receiver, ops);
                return self.temporary_class_method(
                    base,
                    "drop_job",
                    None,
                    args,
                    (receiver, vec![]),
                    ops,
                );
            }
        }
        if !args.is_empty() {
            return Err(base.at.error("drop_job takes no arguments"));
        }
        let Some(ty) = actual else {
            return Err(base.at.error(DROP_JOB_RECEIVER));
        };
        self.drop_job(ty, &base.at, ops).map(Some)
    }

    /// Converts the value of `ty` on the stack to its job-free form. A
    /// failure's job, with its captures, is dropped here rather than when the
    /// error itself is dropped, so a later `?` cleanup cannot wait on its
    /// endpoints. Only tags are copied; nothing allocates.
    pub(super) fn drop_job(&mut self, ty: Ty, at: &Token, ops: &mut Vec<Op>) -> Result<Ty> {
        let Ty::Enum(t) = &ty else {
            return Err(at.error(DROP_JOB_RECEIVER));
        };
        let start = self.locals.len();
        let converted = if t.is_result() {
            let error = t.get().variants[1].fields[0].clone();
            let Some(Ty::Enum(free)) = crate::sum::job_free(&error) else {
                return Err(at.error(format!("{DROP_JOB_RECEIVER}, got {ty}")));
            };
            let Ty::Enum(result) = crate::sum::result(
                t.get().variants[0].fields[0].clone(),
                Ty::Enum(free.clone()),
            ) else {
                unreachable!("Result is an enum")
            };
            let source = self.slot(ty.clone(), at)?;
            ops.push(Op::StoreLocal(source));
            let success = t.get().variants[0].fields[0].clone();
            let ok = vec![
                Op::MoveLocal(source),
                Op::Enum(if success.affine() {
                    crate::sum::EnumOp::Take(t.clone(), 0, 0)
                } else {
                    crate::sum::EnumOp::Field(t.clone(), 0, 0)
                }),
                Op::Enum(crate::sum::EnumOp::New(result.clone(), 0)),
            ];
            let mut err = vec![
                Op::MoveLocal(source),
                Op::Enum(crate::sum::EnumOp::Take(t.clone(), 1, 0)),
            ];
            let Ty::Enum(error) = error else {
                unreachable!("job errors are enums")
            };
            self.forget_job(&error, &free, at, &mut err)?;
            err.push(Op::Enum(crate::sum::EnumOp::New(result.clone(), 1)));
            ops.extend([
                Op::LoadLocal(source),
                Op::Enum(crate::sum::EnumOp::Tag(t.clone())),
                Op::PushInt(Value::I64(0)),
                Op::Eq,
                branch(ok, err),
            ]);
            Ty::Enum(result)
        } else {
            let Some(Ty::Enum(free)) = crate::sum::job_free(&ty) else {
                return Err(at.error(format!("{DROP_JOB_RECEIVER}, got {ty}")));
            };
            self.forget_job(t, &free, at, ops)?;
            Ty::Enum(free)
        };
        self.cleanup(start, ops);
        Ok(converted)
    }

    /// Replaces the job error on the stack by the job-free variant with the
    /// same tag, dropping the job first.
    fn forget_job(
        &mut self,
        error: &crate::nominal::Nominal<crate::sum::EnumType>,
        free: &crate::nominal::Nominal<crate::sum::EnumType>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        let held = self.slot(Ty::Enum(error.clone()), at)?;
        let tag = self.slot(Ty::I64, at)?;
        ops.extend([
            Op::StoreLocal(held),
            Op::LoadLocal(held),
            Op::Enum(crate::sum::EnumOp::Tag(error.clone())),
            Op::StoreLocal(tag),
            Op::DropLocal(held),
        ]);
        let variants = free.get().variants.len();
        let build = |variant: usize| {
            vec![
                Op::PushUnit,
                Op::Enum(crate::sum::EnumOp::New(free.clone(), variant)),
            ]
        };
        let mut chosen = build(variants - 1);
        for variant in (0..variants - 1).rev() {
            chosen = vec![
                Op::LoadLocal(tag),
                Op::PushInt(Value::I64(variant as i64)),
                Op::Eq,
                branch(build(variant), chosen),
            ];
        }
        ops.extend(chosen);
        Ok(())
    }

    pub(super) fn join_thread(
        &mut self,
        base: &Expr,
        method: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let Expression::Name(name) = &ungroup(base).kind else {
            return Err(base.at.error("join requires the task binding from with"));
        };
        if method != "join" || !args.is_empty() {
            return Err(base.at.error("scoped tasks support only join()"));
        }
        let local = &self.names[name];
        let Ty::Task(task) = &local.ty else {
            unreachable!()
        };
        self.mark(&base.at, ops);
        ops.push(Op::MoveLocal(local.slot));
        ops.push(Op::Thread(ThreadOp::Join(task.clone())));
        Ok((task.output != Ty::Unit).then(|| task.output.clone()))
    }
}

/// std.actor's raw entries: `__actor_run(heap, function, args...)` calls a
/// named function with an actor heap current and returns
/// `Result[output, Trapped]`; `__actor_forget(value)` consumes an owned value
/// without releasing it, for values whose heap is discarded as a whole.
impl Lower<'_> {
    pub(super) fn actor_run(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let [heap, worker, args @ ..] = args else {
            return Err(at.error("use __actor_run(heap, function, arguments...)"));
        };
        let actual = self.expr_expected(heap, Some(Ty::I64), ops)?;
        self.same(actual, Some(Ty::I64), &heap.at)?;
        let Expression::Name(function) = &ungroup(worker).kind else {
            return Err(worker.at.error("__actor_run requires a named function"));
        };
        if self.names.contains_key(function) {
            return Err(worker.at.error("__actor_run requires a named function"));
        }
        let begin = ops.len();
        let output = self
            .call_named(function, args, &worker.at, ops)?
            .unwrap_or(Ty::Unit);
        let (index, symbol) = take_call(ops, begin)
            .ok_or_else(|| at.error("__actor_run requires a statically selected function"))?;
        if !output.heap_storable() {
            return Err(at.error(
                "__actor_run results cannot contain references, generators, borrowed/unresolved closures, or scoped tasks",
            ));
        }
        let inputs: Vec<Ty> = self.sigs[&symbol]
            .inputs
            .iter()
            .map(|(_, t)| t.clone())
            .collect();
        // A shared borrow is the parent's node lent to a spawn hook, which
        // runs with the child's heap current.
        for input in &inputs {
            if let Ty::Ref(node, false) = input
                && let Some((barrier, holder)) = node.actor_barrier()
            {
                let place = holder.map_or(String::new(), |holder| format!(" (in `{holder}`)"));
                return Err(at.error(format!(
                    "an actor node cannot hold `{barrier}`{place}: its `spawn` hook reads the node while the child's heap is current; keep text in `Bytes` and pass shared state in messages"
                )));
            }
        }
        let task = Arc::new(Task {
            adapter: format!(
                "__plenty_actor_run_{}_{}_{}",
                self.function_name, at.line, at.column
            ),
            worker: symbol,
            closure: None,
            inputs,
            output,
        });
        if task.bytes() > i32::MAX as usize {
            return Err(at.error("__actor_run task exceeds native stack-layout limit"));
        }
        // Loan uses that followed the call now follow the run.
        let trailing: Vec<_> = ops.drain(index..).collect();
        ops.push(Op::Thread(ThreadOp::Run(task.clone())));
        ops.extend(trailing);
        Ok(Some(task.run_result()))
    }

    pub(super) fn actor_forget(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let [value] = args else {
            return Err(at.error("__actor_forget takes one owned value"));
        };
        let ty = self.value(value, ops)?;
        if !ty.heap_storable() {
            return Err(value
                .at
                .error("__actor_forget requires a wholly owned value"));
        }
        ops.push(Op::Thread(ThreadOp::Forget(ty)));
        Ok(None)
    }
}

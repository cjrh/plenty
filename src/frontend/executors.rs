use super::*;
use crate::executor::{ExecutorOp, Job};

impl Lower<'_> {
    pub(super) fn temporary_executor_method(
        &mut self,
        base: &Expr,
        method: &str,
        args: &[Expr],
        observed: (Ty, Vec<usize>),
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (ty, loans) = observed;
        if matches!(ty, Ty::Future(_)) && method == "result" && !loans.is_empty() {
            return Err(base
                .at
                .error("result() consumes an owned Future, not a borrowed future"));
        }
        let slot = self.slot(ty.clone(), &base.at)?;
        ops.push(Op::StoreLocal(slot));
        self.expression_temps.push(slot);
        let name = format!("__plenty_executor_{slot}");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        let local = Expr {
            at: base.at.clone(),
            kind: Expression::Name(name.clone()),
        };
        let result = self.executor_method(&local, method, args, ops)?;
        self.names.remove(&name);
        Self::end_reads(loans, ops);
        Ok(result)
    }

    pub(super) fn executor_new(
        &mut self,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if args.len() != 2 {
            return Err(
                at.error("use ThreadPoolExecutor(max_workers, queue_capacity), both positive")
            );
        }
        for arg in args {
            self.positive_count(arg, ops)?;
        }
        ops.push(Op::Executor(ExecutorOp::New));
        Ok(Some(ExecutorOp::New.signature().1))
    }

    pub(super) fn executor_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if let Some(Ty::Future(output)) = self.place_type(base) {
            if !matches!(
                (name, args.len()),
                ("result" | "done" | "cancel", 0) | ("wait_timeout" | "wait_timeout_ms", 1)
            ) {
                return Err(base.at.error(
                    "Future supports result(), done(), cancel(), and wait_timeout_ms(timeout_ms)",
                ));
            }
            let loans = if name == "result" {
                let actual = self.value(base, ops)?;
                self.same(Some(actual), Some(Ty::Future(output.clone())), &base.at)?;
                vec![]
            } else {
                self.observe(base, ops)?.1
            };
            let op = match name {
                "result" => ExecutorOp::Result((*output).clone()),
                "done" => ExecutorOp::Done((*output).clone()),
                "wait_timeout" | "wait_timeout_ms" => {
                    let actual = self.expr_expected(&args[0], Some(Ty::U64), ops)?;
                    self.same(actual, Some(Ty::U64), &args[0].at)?;
                    ExecutorOp::WaitTimeout((*output).clone())
                }
                _ => ExecutorOp::Cancel((*output).clone()),
            };
            let output = op.signature().1;
            ops.push(Op::Executor(op));
            Self::end_reads(loans, ops);
            return Ok(Some(output));
        }
        if matches!(self.place_type(base), Some(Ty::Submitter)) {
            return self.submitter_method(base, name, args, ops);
        }
        let (_, loans) = self.observe(base, ops)?;
        let op = if matches!(name, "submit" | "submit_nowait") {
            let job = self.executor_job(base, args, false, ops)?;
            ExecutorOp::Submit(job, name == "submit_nowait")
        } else if matches!(name, "map" | "map_result" | "reduce_tree") {
            self.executor_parallel(args, &base.at, name, ops)?
        } else if name == "shutdown" {
            if let Some(arg) = args.first() {
                return Err(arg.at.error(
                    "shutdown() takes no argument; to drop queued jobs, call pool.cancel_pending() before pool.shutdown()",
                ));
            }
            // The runtime entry still takes its cancel flag; queued jobs are
            // cancelled only through cancel_pending().
            ops.push(Op::PushBool(false));
            ExecutorOp::Shutdown
        } else if name == "cancel_pending" && args.is_empty() {
            ExecutorOp::CancelPending
        } else if name == "submitter" && args.is_empty() {
            ExecutorOp::Submitter
        } else {
            return Err(base.at.error(
                "ThreadPoolExecutor supports submit, submit_nowait, submitter, map, map_result, reduce_tree, cancel_pending, and shutdown",
            ));
        };
        let output = op.signature().1;
        ops.push(Op::Executor(op));
        Self::end_reads(loans, ops);
        if output == Ty::Unit {
            ops.push(Op::Drop);
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }

    /// `pool.submitter()`'s handle: `share()` and `submit_nowait(...)` only.
    /// Blocking submission, futures and shutdown stay with the pool's owner, so
    /// worker code holding a handle never waits for the pool.
    fn submitter_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let (ty, loans) = self.observe(base, ops)?;
        if name == "share" && args.is_empty() {
            Self::end_reads(loans, ops);
            return Ok(Some(ty));
        }
        if name != "submit_nowait" {
            return Err(base.at.error(if name == "submit" {
                "Submitter has no blocking submit: a worker waiting for space in its own pool's queue could wait forever; use submit_nowait and handle Full(job)"
            } else {
                "Submitter supports share() and submit_nowait(job); waiting, results, and shutdown stay with the ThreadPoolExecutor"
            }));
        }
        let job = self.executor_job(base, args, true, ops)?;
        let op = ExecutorOp::Submit(job, true);
        let output = op.signature().1;
        ops.push(Op::Executor(op));
        Self::end_reads(loans, ops);
        Ok(Some(output))
    }

    /// Bind a submitted job, owned closure or named function with its owned
    /// arguments, and push it.
    fn executor_job(
        &mut self,
        base: &Expr,
        args: &[Expr],
        handle: bool,
        ops: &mut Vec<Op>,
    ) -> Result<Arc<Job>> {
        let arguments = ops.len();
        let Some((worker, args)) = args.split_first() else {
            return Err(base
                .at
                .error("submit requires an owned closure or a named function and its arguments"));
        };
        let input = if let Expression::Name(function) = &ungroup(worker).kind {
            if !self.names.contains_key(function) {
                self.executor_named_job(function, args, &worker.at, ops)?
            } else {
                if !args.is_empty() {
                    return Err(worker
                        .at
                        .error("capture inputs in a zero-argument owned job"));
                }
                self.value(worker, ops)?
            }
        } else {
            if !args.is_empty() {
                return Err(worker
                    .at
                    .error("capture inputs in a zero-argument owned job"));
            }
            self.value(worker, ops)?
        };
        let Ty::Closure(closure) = &input else {
            return Err(worker
                .at
                .error("submit requires a concrete owned closure or a statically named function"));
        };
        if !input.heap_storable() || !closure.signature.inputs.is_empty() {
            return Err(worker.at.error("executor jobs require wholly owned captures and no arguments, borrows, or generator frames"));
        }
        let output = closure.signature.output.clone().unwrap_or(Ty::Unit);
        if !output.heap_storable() {
            return Err(worker
                .at
                .error("executor results require a concrete wholly owned type"));
        }
        if handle && output != Ty::Unit {
            return Err(worker.at.error(format!(
                "a Submitter job must return (), not {output}: it has no future, so a result would be dropped unseen; handle its errors in the job or send them on a channel"
            )));
        }
        let job = Arc::new(Job {
            adapter: format!(
                "__plenty_pool_{}_{}_{}",
                self.function_name, base.at.line, base.at.column
            ),
            worker: closure.name.clone(),
            output,
            closure: Some(closure.clone()),
            shared: None,
            input,
            handle,
        });
        self.holds_tokens(arguments, ops);
        Ok(job)
    }

    /// Bind named arguments into the same inline one-shot representation users
    /// can spell explicitly. Recovery errors therefore contain a callable job.
    fn executor_named_job(
        &mut self,
        function: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        let begin = ops.len();
        let output = self.call_named(function, args, at, ops)?;
        let (_, worker) = take_call(ops, begin)
            .ok_or_else(|| at.error("submit requires a statically selected function"))?;
        let captures = self.sigs[&worker].inputs.clone();
        super::threads::generator_argument("submit", function, &captures, args, at)?;
        if captures.iter().any(|(_, ty)| !ty.heap_storable()) {
            return Err(
                at.error("executor arguments must be owned; borrowed work belongs in scoped spawn")
            );
        }
        Self::bind_named_job(worker, output, captures, at, ops).map(Ty::Closure)
    }

    /// Bind the owned arguments already on the stack, for the statically
    /// selected `worker`, into a one-shot zero-argument closure. `spawn` and
    /// `submit` share it, so both return the same callable job on failure.
    pub(super) fn bind_named_job(
        worker: String,
        output: Option<Ty>,
        captures: Vec<(String, Ty)>,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Arc<crate::closure::ClosureType>> {
        let closure = Arc::new(
            crate::closure::ClosureType::new(
                worker,
                Some(at.location().into()),
                crate::op::CallableSig {
                    inputs: vec![],
                    output,
                },
                captures.clone(),
                vec![false; captures.len()],
                true,
            )
            .map_err(|e| at.error(e))?,
        );
        ops.push(Op::ClosureNew(closure.clone()));
        Ok(closure)
    }

    fn executor_parallel(
        &mut self,
        args: &[Expr],
        at: &Token,
        method: &str,
        ops: &mut Vec<Op>,
    ) -> Result<ExecutorOp> {
        let [worker, source] = args else {
            return Err(at.error(format!("use pool.{method}(worker, owned_list_or_range)")));
        };
        let reduce = method == "reduce_tree";
        let owned_worker = match &ungroup(worker).kind {
            Expression::Name(function) if !self.names.contains_key(function) => None,
            _ if reduce => {
                return Err(worker
                    .at
                    .error("reduce_tree requires a statically named function"));
            }
            _ => Some(self.value(worker, ops)?),
        };
        let input = self.value(source, ops)?;
        let (Ty::List(element) | Ty::Range(element)) = &input else {
            return Err(source.at.error(format!(
                "{method} currently consumes an owned list or integer range"
            )));
        };
        let shared = match owned_worker {
            None => None,
            Some(ty) => {
                if matches!(ty, Ty::Ref(..)) {
                    return Err(worker.at.error(format!("{method} workers must own their captures; borrowed work belongs in scoped spawn")));
                }
                let Ty::Closure(closure) = &ty else {
                    return Err(worker.at.error(format!("{method} requires a concrete reusable closure or statically named function; indirect Callable values lack worker eligibility evidence")));
                };
                if closure.once || closure.mutable {
                    return Err(worker.at.error(format!("{method} shares its worker across jobs; use a reusable immutable closure, not a mutable or one-shot closure")));
                }
                if !ty.heap_storable() {
                    return Err(worker.at.error(format!("{method} workers must own their captures; borrowed work belongs in scoped spawn")));
                }
                if closure.signature.inputs != vec![(**element).clone()] {
                    return Err(worker.at.error(format!(
                        "{method} worker must take one owned input of type {element}"
                    )));
                }
                Some(closure.clone())
            }
        };
        let mut arguments = Vec::new();
        let mut names = Vec::new();
        // Probe named workers using typed placeholders without executing the
        // call. Concrete closures already carry their specialized signature.
        for _ in 0..if shared.is_some() {
            0
        } else if reduce {
            2
        } else {
            1
        } {
            let slot = self.slot((**element).clone(), at)?;
            let name = format!("__plenty_executor_item_{slot}");
            self.bind(
                name.clone(),
                Local {
                    slot,
                    ty: (**element).clone(),
                    mutable: false,
                },
            );
            arguments.push(Expr {
                at: at.clone(),
                kind: Expression::Name(name.clone()),
            });
            names.push(name);
        }
        let mut probe = Vec::new();
        let output = if let Some(closure) = &shared {
            Ok(closure.signature.output.clone())
        } else {
            let Expression::Name(function) = &ungroup(worker).kind else {
                unreachable!()
            };
            self.call_named(function, &arguments, &worker.at, &mut probe)
        };
        for name in names {
            self.names.remove(&name);
        }
        let output = output?.ok_or_else(|| {
            worker
                .at
                .error(format!("{method} worker must return a non-unit value"))
        })?;
        if !output.heap_storable() {
            return Err(worker.at.error(format!(
                "{method} results require a concrete wholly owned type"
            )));
        }
        if reduce
            && output != **element
            && !matches!(&output, Ty::Enum(result) if result.is_result() && result.get().variants[0].fields[0] == **element)
        {
            return Err(worker.at.error("reduce_tree worker must take two values of the input element type and return that same type or Result[T, E]"));
        }
        let fallible = method == "map_result";
        if fallible
            && !matches!(&output, Ty::Enum(t)
            if t.propagatable() && !t.is_option() && t.get().variants[0].fields[0] != Ty::Unit)
        {
            return Err(worker
                .at
                .error("map_result worker must return Result[T, E] with a non-unit T"));
        }
        let worker = shared
            .as_ref()
            .map(|closure| closure.name.clone())
            .or_else(|| {
                probe.iter().rev().find_map(|op| match op {
                    Op::Call(name) => Some(name.clone()),
                    _ => None,
                })
            })
            .ok_or_else(|| at.error(format!("{method} requires a statically selected function")))?;
        if shared.is_none()
            && self.sigs[&worker]
                .inputs
                .iter()
                .any(|(_, t)| !t.heap_storable())
        {
            return Err(at.error(format!("{method} workers take their input by value")));
        }
        let adapter = format!(
            "__plenty_pool_{}_{}_{}",
            self.function_name, at.line, at.column
        );
        if reduce {
            let captures = self.sigs[&worker].inputs.clone();
            let closure = Arc::new(
                crate::closure::ClosureType::new(
                    worker.clone(),
                    Some(at.location().into()),
                    crate::op::CallableSig {
                        inputs: vec![],
                        output: Some(output.clone()),
                    },
                    captures,
                    vec![false; 2],
                    true,
                )
                .map_err(|e| at.error(e))?,
            );
            return Ok(ExecutorOp::Reduce(
                Arc::new(Job {
                    adapter,
                    worker,
                    input: Ty::Closure(closure.clone()),
                    output: if output == **element {
                        output
                    } else {
                        let Ty::Enum(result) = &output else {
                            unreachable!()
                        };
                        crate::sum::result(
                            (**element).clone(),
                            crate::sum::anonymous(vec![
                                crate::sum::pool_map_error(),
                                result.get().variants[1].fields[0].clone(),
                            ]),
                        )
                    },
                    closure: Some(closure),
                    shared: None,
                    handle: false,
                }),
                input,
            ));
        }
        Ok(ExecutorOp::Map(
            Arc::new(Job {
                adapter,
                worker,
                input: (**element).clone(),
                output,
                closure: None,
                shared,
                handle: false,
            }),
            input,
            fallible,
        ))
    }
}

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
        self.names.insert(
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
            let actual = self.expr_expected(arg, Some(Ty::U64), ops)?;
            self.same(actual, Some(Ty::U64), &arg.at)?;
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
            if !args.is_empty() || !matches!(name, "result" | "done" | "cancel") {
                return Err(base
                    .at
                    .error("Future supports result(), done(), and cancel()"));
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
                _ => ExecutorOp::Cancel((*output).clone()),
            };
            let output = op.signature().1;
            ops.push(Op::Executor(op));
            Self::end_reads(loans, ops);
            return Ok(Some(output));
        }
        let (_, loans) = self.observe(base, ops)?;
        let op = if matches!(name, "submit" | "submit_nowait") {
            let Some((worker, args)) = args.split_first() else {
                return Err(base.at.error(
                    "submit requires an owned closure or a named function and its arguments",
                ));
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
                return Err(worker.at.error(
                    "submit requires a concrete owned closure or a statically named function",
                ));
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
            let job = Rc::new(Job {
                adapter: format!(
                    "__plenty_pool_{}_{}_{}",
                    self.function_name, base.at.line, base.at.column
                ),
                worker: closure.name.clone(),
                output,
                closure: Some(closure.clone()),
                input,
            });
            ExecutorOp::Submit(job, name == "submit_nowait")
        } else if matches!(name, "map" | "map_result") {
            self.executor_map(args, &base.at, name == "map_result", ops)?
        } else if name == "shutdown" && args.len() <= 1 {
            if let Some(arg) = args.first() {
                let actual = self.expr_expected(arg, Some(Ty::Bool), ops)?;
                self.same(actual, Some(Ty::Bool), &arg.at)?;
            } else {
                ops.push(Op::PushBool(false));
            }
            ExecutorOp::Shutdown
        } else {
            return Err(base.at.error(
                "ThreadPoolExecutor supports submit, submit_nowait, map, map_result, and shutdown",
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
        let index = (begin..ops.len())
            .rfind(|&i| matches!(ops[i], Op::Call(_)))
            .ok_or_else(|| at.error("submit requires a statically selected function"))?;
        let Op::Call(worker) = ops.remove(index) else {
            unreachable!()
        };
        let captures = self.sigs[&worker].inputs.clone();
        if captures.iter().any(|(_, ty)| !ty.heap_storable()) {
            return Err(
                at.error("executor arguments must be owned; borrowed work belongs in scoped spawn")
            );
        }
        let closure = Rc::new(
            crate::closure::ClosureType::new(
                worker,
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
        Ok(Ty::Closure(closure))
    }

    fn executor_map(
        &mut self,
        args: &[Expr],
        at: &Token,
        fallible: bool,
        ops: &mut Vec<Op>,
    ) -> Result<ExecutorOp> {
        let [worker, source] = args else {
            return Err(at.error("use pool.map(named_function, owned_list_or_range)"));
        };
        let Expression::Name(function) = &ungroup(worker).kind else {
            return Err(worker
                .at
                .error("map currently requires a statically named function"));
        };
        if self.names.contains_key(function) {
            return Err(worker.at.error("map currently requires a statically named function; submit owned closures individually"));
        }
        let input = self.value(source, ops)?;
        let (Ty::List(element) | Ty::Range(element)) = &input else {
            return Err(source
                .at
                .error("map currently consumes an owned list or integer range"));
        };
        // Specialize and check the ordinary named call with a typed placeholder.
        // Its operations never execute; only the selected signature and body
        // enter the compiler's usual monomorphization/effect-checking pipeline.
        let slot = self.slot((**element).clone(), at)?;
        let name = format!("__executor_item_{slot}");
        let saved = self.names.insert(
            name.clone(),
            Local {
                slot,
                ty: (**element).clone(),
                mutable: false,
            },
        );
        let argument = Expr {
            at: at.clone(),
            kind: Expression::Name(name.clone()),
        };
        let mut probe = Vec::new();
        let output = self.call_named(function, &[argument], &worker.at, &mut probe)?;
        self.names.remove(&name);
        if let Some(saved) = saved {
            self.names.insert(name, saved);
        }
        let output =
            output.ok_or_else(|| worker.at.error("map worker must return a non-unit value"))?;
        if !output.heap_storable() {
            return Err(worker
                .at
                .error("map results require a concrete wholly owned type"));
        }
        if fallible
            && !matches!(&output, Ty::Enum(t)
            if t.propagatable() && !t.is_option() && t.get().variants[0].fields[0] != Ty::Unit)
        {
            return Err(worker
                .at
                .error("map_result worker must return Result[T, E] with a non-unit T"));
        }
        let worker = probe
            .iter()
            .rev()
            .find_map(|op| match op {
                Op::Call(name) => Some(name.clone()),
                _ => None,
            })
            .ok_or_else(|| at.error("map requires a statically selected function"))?;
        if self.sigs[&worker]
            .inputs
            .iter()
            .any(|(_, t)| !t.heap_storable())
        {
            return Err(at.error("map workers take their input by value"));
        }
        Ok(ExecutorOp::Map(
            Rc::new(Job {
                adapter: format!(
                    "__plenty_pool_{}_{}_{}",
                    self.function_name, at.line, at.column
                ),
                worker,
                input: (**element).clone(),
                output,
                closure: None,
            }),
            input,
            fallible,
        ))
    }
}

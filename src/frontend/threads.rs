//! Lexically scoped native jobs reuse the existing context cleanup obligations.
use super::*;
use crate::threading::{Task, ThreadOp};

pub(super) fn spawn_expression(manager: &Expr) -> Option<(&Expr, bool)> {
    let (call, propagate) = match &ungroup(manager).kind {
        Expression::Try(call) => (&**call, true),
        Expression::Method(call, name, args) if name == "unwrap" && args.is_empty() => {
            (&**call, false)
        }
        _ => return None,
    };
    matches!(&ungroup(call).kind, Expression::Call(name, _) if name == "spawn")
        .then_some((ungroup(call), propagate))
}

impl Lower<'_> {
    pub(super) fn with_thread(
        &mut self,
        call: &Expr,
        propagate: bool,
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
        let (worker_name, closure, inputs, output, loans) = match &ungroup(worker).kind {
            Expression::Name(function) if !self.names.contains_key(function) => {
                let begin = ops.len();
                let output = self
                    .call_named(function, args, &worker.at, ops)?
                    .unwrap_or(Ty::Unit);
                let index = (begin..ops.len())
                    .rfind(|&i| matches!(ops[i], Op::Call(_)))
                    .ok_or_else(|| {
                        call.at
                            .error("spawn requires a statically selected function")
                    })?;
                let Op::Call(symbol) = ops.remove(index) else {
                    unreachable!()
                };
                let inputs: Vec<_> = self.sigs[&symbol]
                    .inputs
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect();
                if inputs.iter().any(Ty::affine) {
                    return Err(call.at.error("spawn borrows mutable owners; pass & or &mut, or borrow a reusable closure, so a failed start preserves your job"));
                }
                let loans = ops[index..]
                    .iter()
                    .filter_map(|op| match op {
                        Op::UseLoan(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
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
                return Err(worker.at.error(
                    "spawn requires a named function or an explicitly borrowed reusable closure",
                ))
            }
        };
        if !output.heap_storable() {
            return Err(call.at.error(
                "worker results cannot contain references, generators, borrowed/unresolved closures, or scoped tasks",
            ));
        }
        let task = Rc::new(Task {
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
        ops.push(Op::Thread(ThreadOp::Start(task.clone(), slot)));
        let Ty::Enum(source) = crate::sum::result(Ty::Unit, crate::sum::thread_error()) else {
            unreachable!()
        };
        if propagate {
            let Some(Some(Ty::Enum(target))) = self.return_type.clone() else {
                return Err(call.at.error("`?` requires a function returning Result"));
            };
            if !target.propagatable()
                || target.is_option()
                || (!target.discards_error()
                    && target.get().variants[1].fields != source.get().variants[1].fields)
            {
                return Err(call
                    .at
                    .error("spawn propagation requires Result with ThreadError or Failure"));
            }
            let mut cleanup = Vec::new();
            self.cleanup(0, &mut cleanup);
            ops.push(Op::Try {
                source,
                target,
                cleanup: cleanup.into(),
            });
        } else {
            ops.push(Op::Enum(crate::sum::EnumOp::Unwrap(source)));
        }
        ops.push(Op::Drop);
        let mut exit = vec![Op::Thread(ThreadOp::Finish(task.clone(), slot))];
        exit.extend(loans.into_iter().map(Op::UseLoan));
        self.contexts.push(contexts::Context {
            slot,
            exit,
            loan: None,
            thread: true,
        });
        if let Some(name) = name {
            if self.names.contains_key(name) {
                return Err(call.at.error(format!("duplicate binding `{name}`")));
            }
            self.names.insert(
                name.into(),
                Local {
                    slot,
                    ty: Ty::Task(task),
                    mutable: false,
                },
            );
        }
        let result = self.block_inner(body, ops, false)?;
        if matches!(result, BlockResult::Continues(_)) {
            self.cleanup(start, ops);
        }
        self.contexts.pop();
        self.names = saved;
        Ok(result)
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
        ops.push(Op::MoveLocal(
            local.slot,
            format!("{}:{}: task `{name}`", base.at.line, base.at.column),
        ));
        ops.push(Op::Thread(ThreadOp::Join(task.clone())));
        Ok((task.output != Ty::Unit).then(|| task.output.clone()))
    }
}

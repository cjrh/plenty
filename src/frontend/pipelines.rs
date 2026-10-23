//! Owner-local iterator plans. Initialization runs at construction; only the
//! condition, item body, and step are spliced into the consuming loop.
use super::collections::Iteration;
use super::*;

#[derive(Clone)]
pub(super) struct Pipeline {
    iteration: Iteration,
    pub(super) item: Local,
    resources: Vec<u8>,
    loans: Vec<usize>,
    depth: usize,
}

impl Lower<'_> {
    pub(super) fn is_pipeline(&self, e: &Expr) -> bool {
        match &ungroup(e).kind {
            Expression::Name(n) => matches!(self.names.get(n).map(|l| &l.ty), Some(Ty::Iter(..))),
            Expression::Call(n, _) => {
                n == "iter"
                    && !self.names.contains_key(n)
                    && !self.sigs.contains_key(n)
                    && !self.generics.templates.contains_key(n)
                    && !self.aliases.contains_key(n)
            }
            Expression::Method(base, name, _, _) => {
                matches!(
                    name.as_str(),
                    "map" | "filter" | "take" | "skip" | "take_while"
                ) && self.is_pipeline(base)
            }
            _ => false,
        }
    }

    pub(super) fn pipeline(&mut self, e: &Expr, ops: &mut Vec<Op>) -> Result<Pipeline> {
        if self.yield_type.is_some() {
            return Err(e.at.error("iterator pipelines cannot be held across yield"));
        }
        let start = self.locals.len();
        let ops_start = ops.len();
        let saved = self.names.clone();
        let mut plan = match &ungroup(e).kind {
            Expression::Name(n) => {
                let local = self.names[n].clone();
                let Ty::Iter(_, id) = local.ty else {
                    unreachable!()
                };
                self.mark(&e.at, ops);
                ops.extend([Op::MoveLocal(local.slot), Op::Drop]);
                self.pipelines
                    .get(id)
                    .cloned()
                    .ok_or_else(|| e.at.error("iterator pipeline has no local source plan"))?
            }
            Expression::Call(_, args) => {
                let [source] = args.as_slice() else {
                    return Err(e.at.error("iter requires one iterable"));
                };
                let name = format!("<pipeline item {start}>");
                let iteration = self.iteration(std::slice::from_ref(&name), source, None, ops)?;
                let item = self.names[&name].clone();
                if matches!(&item.ty, Ty::Enum(t) if t.tuple()) {
                    return Err(e.at.error(
                        "pipeline pair items are not implemented yet; use an unpacking for loop",
                    ));
                }
                Pipeline {
                    iteration,
                    item,
                    resources: vec![],
                    loans: vec![],
                    depth: 0,
                }
            }
            Expression::Method(base, method, args, _) => {
                let mut plan = self.pipeline(base, ops)?;
                if plan.depth >= 64 {
                    return Err(e.at.error(
                        "iterator pipeline nesting exceeds the implementation limit of 64",
                    ));
                }
                plan.depth += 1;
                let [argument] = args.as_slice() else {
                    return Err(e.at.error(format!("{method} requires one argument")));
                };
                if matches!(method.as_str(), "map" | "filter" | "take_while") {
                    let callback = self.pipeline_callback(argument, ops)?;
                    let item = self.pipeline_item(&plan, &e.at);
                    let item = if method == "map" || matches!(plan.item.ty, Ty::Ref(..)) {
                        item
                    } else {
                        Expr {
                            at: e.at.clone(),
                            kind: Expression::Unary("&".into(), Box::new(item)),
                        }
                    };
                    let output =
                        self.pipeline_call(&callback, &[item], &mut plan.iteration.body)?;
                    if method == "map" {
                        let ty = output
                            .ok_or_else(|| e.at.error("map callbacks must return a value"))?;
                        if matches!(ty, Ty::Ref(..)) {
                            return Err(e
                                .at
                                .error("pipeline map callbacks cannot return references yet"));
                        }
                        if matches!(&ty, Ty::Enum(t) if t.tuple()) {
                            return Err(e.at.error("pipeline pair items are not implemented yet; use an unpacking for loop"));
                        }
                        let slot = self.slot(ty.clone(), &e.at)?;
                        plan.iteration.body.push(Op::StoreLocal(slot));
                        if ty.managed() {
                            plan.iteration.step.push(Op::DropLocal(slot));
                        }
                        plan.iteration.target = slot;
                        plan.item = Local {
                            slot,
                            ty,
                            mutable: false,
                        };
                    } else {
                        self.same(output, Some(Ty::Bool), &argument.at)?;
                        let mut rejected = if method == "filter" {
                            plan.iteration.step.clone()
                        } else {
                            vec![]
                        };
                        rejected.push(if method == "filter" {
                            Op::Continue
                        } else {
                            Op::Break
                        });
                        plan.iteration.body.push(branch(vec![], rejected));
                    }
                } else {
                    let actual = self.expr_expected(argument, Some(Ty::I64), ops)?;
                    self.same(actual, Some(Ty::I64), &argument.at)?;
                    let remaining = self.slot(Ty::I64, &e.at)?;
                    ops.push(Op::StoreLocal(remaining));
                    let positive =
                        vec![Op::LoadLocal(remaining), Op::PushInt(Value::I64(0)), Op::Gt];
                    let decrement = vec![
                        Op::LoadLocal(remaining),
                        Op::PushInt(Value::I64(1)),
                        Op::Sub,
                        Op::StoreLocal(remaining),
                    ];
                    if method == "take" {
                        let mut condition = positive;
                        let mut absent = plan.iteration.absent.clone();
                        absent.push(Op::PushBool(false));
                        condition.push(branch(plan.iteration.condition, absent));
                        plan.iteration.condition = condition;
                        plan.iteration.body.extend(decrement);
                    } else {
                        let mut skipped = decrement;
                        skipped.extend(plan.iteration.step.clone());
                        skipped.push(Op::Continue);
                        plan.iteration.body.extend(positive);
                        plan.iteration.body.push(branch(skipped, vec![]));
                    }
                }
                plan
            }
            _ => return Err(e.at.error("expected an iterator pipeline")),
        };
        plan.resources
            .extend((start..self.locals.len()).map(|i| (self.parameters + i) as u8));
        plan.resources.sort_unstable();
        plan.resources.dedup();
        plan.loans
            .extend(ops[ops_start..].iter().filter_map(|op| match op {
                Op::Loan(loan) => Some(loan.id),
                _ => None,
            }));
        plan.loans.sort_unstable();
        plan.loans.dedup();
        self.names = saved;
        Ok(plan)
    }

    fn pipeline_item(&mut self, plan: &Pipeline, at: &Token) -> Expr {
        let name = format!("<pipeline value {}>", plan.item.slot);
        self.bind(name.clone(), plan.item.clone());
        Expr {
            at: at.clone(),
            kind: Expression::Name(name),
        }
    }

    fn pipeline_callback(&mut self, callback: &Expr, ops: &mut Vec<Op>) -> Result<Expr> {
        if let Expression::Name(n) = &ungroup(callback).kind
            && self.sigs.contains_key(n)
            && !self.names.contains_key(n)
        {
            return Ok(callback.clone());
        }
        let ty = if let Expression::Anonymous(function) = &ungroup(callback).kind {
            self.closure_value(function, &callback.at, ops)?
        } else {
            self.value(callback, ops)?
        };
        if !matches!(&ty, Ty::Closure(c) if !c.once) {
            return Err(callback
                .at
                .error("pipeline callbacks require a named function or reusable closure"));
        }
        let slot = self.slot(ty.clone(), &callback.at)?;
        ops.push(Op::StoreLocal(slot));
        let name = format!("<pipeline callback {slot}>");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: true,
            },
        );
        Ok(Expr {
            at: callback.at.clone(),
            kind: Expression::Name(name),
        })
    }

    fn pipeline_call(&mut self, callback: &Expr, args: &[Expr], ops: &mut Vec<Op>) -> Result<Type> {
        if let Expression::Name(n) = &callback.kind
            && self.sigs.contains_key(n)
            && !self.names.contains_key(n)
        {
            return self.expr(
                &Expr {
                    at: callback.at.clone(),
                    kind: Expression::Call(n.clone(), args.to_vec()),
                },
                ops,
            );
        }
        self.call_value(callback, args, ops)
    }

    pub(super) fn finish_pipeline(&self, plan: &Pipeline, ops: &mut Vec<Op>) {
        ops.extend(plan.iteration.cleanup.iter().cloned());
        ops.extend(plan.loans.iter().copied().map(Op::UseLoan));
        for &slot in plan.resources.iter().rev() {
            if self.locals[slot as usize - self.parameters].managed() {
                ops.push(Op::DropLocal(slot));
            }
        }
    }

    pub(super) fn pipeline_iteration(
        &mut self,
        names: &[String],
        e: &Expr,
        ops: &mut Vec<Op>,
    ) -> Result<Iteration> {
        let plan = self.pipeline(e, ops)?;
        if names.len() != 1 {
            return Err(e.at.error("pipeline pair bindings are not implemented yet"));
        }
        self.bind(names[0].clone(), plan.item.clone());
        // Consumers own every source/callback slot, including slots initialized
        // earlier by a local pipeline binding.
        let mut cleanup = Vec::new();
        self.finish_pipeline(&plan, &mut cleanup);
        let mut iteration = plan.iteration;
        iteration.cleanup = cleanup;
        Ok(iteration)
    }

    pub(super) fn pipeline_terminal(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let start = self.locals.len();
        let saved = self.names.clone();
        let plan = self.pipeline(base, ops)?;
        let item = self.pipeline_item(&plan, &base.at);
        let mut body = plan.iteration.body.clone();
        let output = match (name, args) {
            ("fold", [initial, callback]) => {
                let ty = self.value(initial, ops)?;
                let slot = self.slot(ty.clone(), &initial.at)?;
                ops.push(Op::StoreLocal(slot));
                let acc = format!("<pipeline accumulator {slot}>");
                self.bind(
                    acc.clone(),
                    Local {
                        slot,
                        ty: ty.clone(),
                        mutable: true,
                    },
                );
                let callback = self.pipeline_callback(callback, ops)?;
                let actual = self.pipeline_call(
                    &callback,
                    &[
                        Expr {
                            at: initial.at.clone(),
                            kind: Expression::Name(acc),
                        },
                        item,
                    ],
                    &mut body,
                )?;
                self.same(actual, Some(ty.clone()), &callback.at)?;
                body.push(Op::StoreLocal(slot));
                Some((ty, slot))
            }
            ("count", []) => {
                let slot = self.slot(Ty::I64, &base.at)?;
                ops.extend([Op::PushInt(Value::I64(0)), Op::StoreLocal(slot)]);
                body.extend([
                    Op::LoadLocal(slot),
                    Op::PushInt(Value::I64(1)),
                    Op::Add,
                    Op::StoreLocal(slot),
                ]);
                Some((Ty::I64, slot))
            }
            ("any" | "all" | "find", [callback]) => {
                let callback = self.pipeline_callback(callback, ops)?;
                let predicate_item = if matches!(plan.item.ty, Ty::Ref(..)) {
                    item
                } else {
                    Expr {
                        at: base.at.clone(),
                        kind: Expression::Unary("&".into(), Box::new(item)),
                    }
                };
                let actual = self.pipeline_call(&callback, &[predicate_item], &mut body)?;
                self.same(actual, Some(Ty::Bool), &callback.at)?;
                if name == "find" {
                    if let Some(barrier) = plan.item.ty.storage_barrier() {
                        return Err(base
                            .at
                            .error(barrier.message("a find result; map to an owned value first")));
                    }
                    let ty = crate::sum::option(plan.item.ty.clone());
                    let Ty::Enum(option) = &ty else {
                        unreachable!()
                    };
                    let slot = self.slot(ty.clone(), &base.at)?;
                    ops.extend([
                        Op::Enum(crate::sum::EnumOp::New(option.clone(), 0)),
                        Op::StoreLocal(slot),
                    ]);
                    body.push(branch(
                        vec![
                            if plan.item.ty.affine() {
                                Op::MoveLocal(plan.item.slot)
                            } else {
                                Op::LoadLocal(plan.item.slot)
                            },
                            Op::Enum(crate::sum::EnumOp::New(option.clone(), 1)),
                            Op::StoreLocal(slot),
                            Op::Break,
                        ],
                        vec![],
                    ));
                    Some((ty, slot))
                } else {
                    let slot = self.slot(Ty::Bool, &base.at)?;
                    let identity = name == "all";
                    ops.extend([Op::PushBool(identity), Op::StoreLocal(slot)]);
                    if identity {
                        body.push(Op::Not);
                    }
                    body.push(branch(
                        vec![Op::PushBool(!identity), Op::StoreLocal(slot), Op::Break],
                        vec![],
                    ));
                    Some((Ty::Bool, slot))
                }
            }
            ("reduce", [callback]) => {
                if let Some(barrier) = plan.item.ty.storage_barrier() {
                    return Err(base
                        .at
                        .error(barrier.message("a reduce result; map to an owned value first")));
                }
                let callback = self.pipeline_callback(callback, ops)?;
                let ty = crate::sum::option(plan.item.ty.clone());
                let Ty::Enum(option) = &ty else {
                    unreachable!()
                };
                let slot = self.slot(ty.clone(), &base.at)?;
                let acc = self.slot(plan.item.ty.clone(), &base.at)?;
                let acc_name = format!("<pipeline accumulator {acc}>");
                self.bind(
                    acc_name.clone(),
                    Local {
                        slot: acc,
                        ty: plan.item.ty.clone(),
                        mutable: true,
                    },
                );
                ops.extend([
                    Op::Enum(crate::sum::EnumOp::New(option.clone(), 0)),
                    Op::StoreLocal(slot),
                ]);
                body.extend([
                    Op::LoadLocal(slot),
                    Op::Enum(crate::sum::EnumOp::Tag(option.clone())),
                    Op::PushInt(Value::I64(1)),
                    Op::Eq,
                ]);
                let mut combine = vec![
                    Op::MoveLocal(slot),
                    Op::Enum(crate::sum::EnumOp::Take(option.clone(), 1, 0)),
                    Op::StoreLocal(acc),
                ];
                let actual = self.pipeline_call(
                    &callback,
                    &[
                        Expr {
                            at: base.at.clone(),
                            kind: Expression::Name(acc_name),
                        },
                        item,
                    ],
                    &mut combine,
                )?;
                self.same(actual, Some(plan.item.ty.clone()), &callback.at)?;
                body.push(branch(
                    combine,
                    vec![if plan.item.ty.affine() {
                        Op::MoveLocal(plan.item.slot)
                    } else {
                        Op::LoadLocal(plan.item.slot)
                    }],
                ));
                body.extend([
                    Op::Enum(crate::sum::EnumOp::New(option.clone(), 1)),
                    Op::StoreLocal(slot),
                ]);
                Some((ty, slot))
            }
            ("for_each", [callback]) => {
                let callback = self.pipeline_callback(callback, ops)?;
                let actual = self.pipeline_call(&callback, &[item], &mut body)?;
                self.same(actual, None, &callback.at)?;
                None
            }
            _ => {
                return Err(base.at.error(format!(
                    "unsupported pipeline terminal or arguments: {name}"
                )));
            }
        };
        body.extend(plan.iteration.step.clone());
        ops.push(Op::Loop {
            condition: plan.iteration.condition.clone().into(),
            body: body.into(),
        });
        if let Some((ty, slot)) = &output {
            ops.push(if ty.affine() {
                Op::MoveLocal(*slot)
            } else {
                Op::LoadLocal(*slot)
            });
        }
        self.finish_pipeline(&plan, ops);
        self.cleanup(start, ops);
        self.names = saved;
        Ok(output.map(|(ty, _)| ty))
    }
}

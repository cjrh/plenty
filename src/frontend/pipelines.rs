//! Owner-local iterator plans. Initialization runs at construction; only the
//! condition, item body, and step are spliced into the consuming loop.
use super::collections::Iteration;
use super::*;

mod generator_bridge;
mod splice;
pub(super) use splice::Callback;

fn result_payload(ty: &Ty) -> Option<Ty> {
    match ty {
        Ty::Enum(result) if result.is_result() => Some(result.get().variants[0].fields[0].clone()),
        _ => None,
    }
}

#[derive(Clone)]
pub(super) struct Pipeline {
    iteration: Iteration,
    // The type is always the logical item type. With `pair`, its components
    // live in separate locals: `item.slot` is not a stored tuple until packed.
    pub(super) item: Local,
    pair: Option<[Local; 2]>,
    resources: Vec<u8>,
    initialized: std::collections::BTreeSet<u8>,
    // Slots that construction leaves empty, such as `flat_map` inner state.
    empty: Vec<u8>,
    // A `chain` flag that a terminal may split into one loop per side. Only
    // single-source adapters may follow it: a `zip` or `flat_map` pull runs
    // other side effects before the chain's condition.
    split: Option<u8>,
    owned: bool,
    loans: Vec<usize>,
    depth: usize,
}

impl Lower<'_> {
    pub(super) fn is_generator_source(&self, e: &Expr) -> bool {
        let hint = |e: &Expr| self.place_type(e).or_else(|| self.expression_type_hint(e));
        if matches!(hint(e), Some(Ty::Generator(_))) {
            return true;
        }
        let success_is_generator = |e: &Expr| {
            matches!(hint(e), Some(Ty::Enum(sum))
            if (sum.is_result() || sum.is_option())
                && matches!(sum.get().variants[usize::from(sum.is_option())].fields.first(), Some(Ty::Generator(_))))
        };
        match &ungroup(e).kind {
            Expression::Call(name, _) if !self.names.contains_key(name) => self
                .generics
                .templates
                .get(name)
                .is_some_and(|function| function.output.name.as_deref() == Some("Generator")),
            Expression::Collection { kind, .. } => kind == "generator",
            Expression::Try(inner) => success_is_generator(inner),
            Expression::Conditional { yes, no, .. } => {
                self.is_generator_source(yes) && self.is_generator_source(no)
            }
            Expression::Method(base, name, args, _) => {
                if name == "generator" && self.is_pipeline(base) {
                    return true;
                }
                if name == "unwrap" && args.is_empty() && success_is_generator(base) {
                    return true;
                }
                if name == "new"
                    && let Expression::Name(function) = &ungroup(base).kind
                {
                    return !self.names.contains_key(function)
                        && self.sigs.get(function).is_some_and(|sig| {
                            matches!(sig.outputs.first(), Some(Ty::Generator(_)))
                        });
                }
                let owner = match hint(base).map(classes::unbox) {
                    Some(Ty::Class(class)) => class.name.clone(),
                    Some(Ty::Enum(sum)) => sum.name.clone(),
                    _ => return false,
                };
                let method = crate::record::method(&owner, name);
                self.sigs
                    .get(&method)
                    .is_some_and(|sig| matches!(sig.outputs.first(), Some(Ty::Generator(_))))
                    || self
                        .generics
                        .templates
                        .get(&method)
                        .is_some_and(|function| {
                            function.output.name.as_deref() == Some("Generator")
                        })
            }
            _ => false,
        }
    }

    pub(super) fn is_pipeline(&self, e: &Expr) -> bool {
        if self.is_clist_column(e) {
            return true;
        }
        match &ungroup(e).kind {
            Expression::Name(n) => matches!(self.names.get(n).map(|l| &l.ty), Some(Ty::Iter(..))),
            Expression::Call(n, _) => {
                matches!(n.as_str(), "iter" | "enumerate" | "zip")
                    && !self.names.contains_key(n)
                    && !self.sigs.contains_key(n)
                    && !self.generics.templates.contains_key(n)
                    && !self.aliases.contains_key(n)
            }
            Expression::Method(base, name, _, _) => {
                if name == "items"
                    && super::collections::dictionary_items(e).is_some_and(|(base, _)| {
                        self.place_type(base)
                            .or_else(|| self.expression_type_hint(base))
                            .map(classes::unbox)
                            .is_some_and(|t| matches!(t, Ty::Dict(..)))
                    })
                {
                    return true;
                }
                matches!(
                    name.as_str(),
                    "map"
                        | "filter"
                        | "take"
                        | "skip"
                        | "take_while"
                        | "filter_value"
                        | "enumerate"
                        | "zip"
                        | "chain"
                        | "flat_map"
                ) && (self.is_pipeline(base) || self.is_generator_source(base))
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
            Expression::Call(function, args) if function != "iter" => {
                if function == "enumerate" && !(1..=2).contains(&args.len()) {
                    return Err(e
                        .at
                        .error("enumerate requires an iterable and optional i64 start"));
                }
                if function == "zip" && args.len() != 2 {
                    return Err(e.at.error("zip requires two iterables"));
                }
                let (source, rest) = args
                    .split_first()
                    .ok_or_else(|| e.at.error("adapter requires a source"))?;
                let mut base = self.pipeline_source(source, ops)?;
                if base.depth >= 64 {
                    return Err(e.at.error(
                        "iterator pipeline nesting exceeds the implementation limit of 64",
                    ));
                }
                base.depth += 1;
                self.pipeline_pair_adapter(base, function, rest, &e.at, ops)?
            }
            Expression::Call(_, args) => {
                let [source] = args.as_slice() else {
                    return Err(e.at.error("iter requires one iterable"));
                };
                self.pipeline_source(source, ops)?
            }
            Expression::Method(_, method, _, _) if method == "items" || self.is_clist_column(e) => {
                self.pipeline_raw_source(e, ops)?
            }
            Expression::Method(base, method, args, _) => {
                let mut plan = self.pipeline_source(base, ops)?;
                if plan.depth >= 64 {
                    return Err(e.at.error(
                        "iterator pipeline nesting exceeds the implementation limit of 64",
                    ));
                }
                plan.depth += 1;
                if matches!(method.as_str(), "enumerate" | "zip") {
                    plan = self.pipeline_pair_adapter(plan, method, args, &e.at, ops)?;
                } else if method == "chain" {
                    plan = self.pipeline_chain(plan, args, &e.at, ops)?;
                } else if method == "flat_map" {
                    plan = self.pipeline_flat_map(plan, args, &e.at, ops)?;
                } else if method == "filter_value" {
                    self.pipeline_filter_value(&mut plan, args, &e.at, ops)?;
                } else {
                    let [argument] = args.as_slice() else {
                        return Err(e.at.error(format!("{method} requires one argument")));
                    };
                    if matches!(method.as_str(), "map" | "filter" | "take_while") {
                        let callback = self.pipeline_callback(argument, ops)?;
                        plan.owned &= !self.callback_borrows(&callback);
                        let items = self.pipeline_arguments(&plan, method != "map", &e.at);
                        let output =
                            self.pipeline_call(&callback, &items, &mut plan.iteration.body)?;
                        if method == "map" {
                            let ty = output
                                .ok_or_else(|| e.at.error("map callbacks must return a value"))?;
                            if matches!(ty, Ty::Ref(..)) {
                                return Err(e
                                    .at
                                    .error("pipeline map callbacks cannot return references yet"));
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
                            plan.pair = None;
                            self.pipeline_split_pair(&mut plan, &e.at)?;
                        } else {
                            plan.iteration.exact_len = None;
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
                        if let Some(length) = plan.iteration.exact_len.take() {
                            let mut less = vec![Op::LoadLocal(remaining)];
                            less.extend(length.clone());
                            less.push(Op::Lt);
                            let mut exact =
                                vec![Op::LoadLocal(remaining), Op::PushInt(Value::I64(0)), Op::Gt];
                            if method == "take" {
                                less.push(branch(vec![Op::LoadLocal(remaining)], length));
                                exact.push(branch(less, vec![Op::PushInt(Value::I64(0))]));
                            } else {
                                let mut rest = length.clone();
                                rest.extend([Op::LoadLocal(remaining), Op::Sub]);
                                less.push(branch(rest, vec![Op::PushInt(Value::I64(0))]));
                                exact.push(branch(less, length));
                            }
                            plan.iteration.exact_len = Some(exact);
                        }
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
                }
                plan
            }
            _ => return Err(e.at.error("expected an iterator pipeline")),
        };
        plan.resources
            .extend((start..self.locals.len()).map(|i| (self.parameters + i) as u8));
        plan.resources.sort_unstable();
        plan.resources.dedup();
        generator_bridge::initialized_after(&ops[ops_start..], &mut plan.initialized);
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

    fn pipeline_source(&mut self, source: &Expr, ops: &mut Vec<Op>) -> Result<Pipeline> {
        if self.yield_type.is_some() {
            return Err(source
                .at
                .error("iterator pipelines cannot be held across yield"));
        }
        if self.is_pipeline(source) {
            return self.pipeline(source, ops);
        }
        self.pipeline_raw_source(source, ops)
    }

    fn pipeline_raw_source(&mut self, source: &Expr, ops: &mut Vec<Op>) -> Result<Pipeline> {
        let start = self.locals.len();
        let ops_start = ops.len();
        let name = format!("<pipeline item {}>", self.locals.len());
        // Dictionary item views cannot form a stored tuple of references.
        let paired = super::collections::dictionary_items(source).is_some_and(|(base, _)| {
            !self
                .place_type(base)
                .or_else(|| self.expression_type_hint(base))
                .map(classes::unbox)
                .as_ref()
                .is_some_and(classes::inherent_receiver)
        }) && {
            let mut expression = ungroup(source);
            if let Expression::Unary(_, inner) = &expression.kind {
                expression = ungroup(inner);
            }
            matches!(&expression.kind, Expression::Method(_, n, _, _) if n == "items")
        };
        let names = if paired {
            vec![format!("{name} key"), format!("{name} value")]
        } else {
            vec![name]
        };
        let iteration = if paired {
            let (base, mutable) = super::collections::dictionary_items(source).unwrap();
            self.dictionary_iteration(&names, base, mutable, false, ops)?
        } else {
            self.iteration(&names, source, None, ops)?
        };
        let pair = paired.then(|| [self.names[&names[0]].clone(), self.names[&names[1]].clone()]);
        let item = if let Some(pair) = &pair {
            Local {
                slot: pair[0].slot,
                ty: crate::sum::tuple(pair.iter().map(|p| p.ty.clone()).collect()),
                mutable: false,
            }
        } else {
            self.names[&names[0]].clone()
        };
        let owned = !self.is_clist_column(source)
            && !item.ty.contains_reference()
            && pair
                .as_ref()
                .is_none_or(|fields| fields.iter().all(|field| !field.ty.contains_reference()));
        let mut plan = Pipeline {
            iteration,
            item,
            pair,
            resources: vec![],
            initialized: Default::default(),
            empty: vec![],
            split: None,
            owned,
            loans: vec![],
            depth: 0,
        };
        self.pipeline_split_pair(&mut plan, &source.at)?;
        plan.resources
            .extend((start..self.locals.len()).map(|i| (self.parameters + i) as u8));
        generator_bridge::initialized_after(&ops[ops_start..], &mut plan.initialized);
        Ok(plan)
    }

    fn pipeline_split_pair(&mut self, plan: &mut Pipeline, at: &Token) -> Result<()> {
        if plan.pair.is_some() {
            return Ok(());
        }
        let Ty::Enum(tuple) = &plan.item.ty else {
            return Ok(());
        };
        if !tuple.tuple() || tuple.get().variants[0].fields.len() != 2 {
            return Ok(());
        }
        let mut fields = Vec::new();
        for ty in &tuple.get().variants[0].fields {
            let slot = self.slot(ty.clone(), at)?;
            fields.push(Local {
                slot,
                ty: ty.clone(),
                mutable: false,
            });
            if ty.managed() {
                plan.iteration.step.push(Op::DropLocal(slot));
            }
        }
        plan.iteration
            .body
            .extend([Op::MoveLocal(plan.item.slot), Op::Split(tuple.clone(), 0)]);
        plan.iteration
            .body
            .extend(fields.iter().rev().map(|f| Op::StoreLocal(f.slot)));
        plan.pair = Some(fields.try_into().unwrap_or_else(|_| unreachable!()));
        Ok(())
    }

    /// Reconstitute an owned pair only at a consumer that stores the item.
    fn pipeline_pack_pair(&mut self, plan: &mut Pipeline, at: &Token) -> Result<()> {
        let Some(fields) = plan.pair.take() else {
            return Ok(());
        };
        if let Some(barrier) = fields.iter().find_map(|f| f.ty.storage_barrier()) {
            return Err(
                at.error(barrier.message("a pipeline item tuple; map to owned values first"))
            );
        }
        let ty = crate::sum::tuple(fields.iter().map(|f| f.ty.clone()).collect());
        let Ty::Enum(tuple) = &ty else { unreachable!() };
        let slot = self.slot(ty.clone(), at)?;
        for field in fields {
            plan.iteration.body.push(if field.ty.affine() {
                Op::MoveLocal(field.slot)
            } else {
                Op::LoadLocal(field.slot)
            });
        }
        plan.iteration.body.extend([
            Op::Enum(crate::sum::EnumOp::New(tuple.clone(), 0)),
            Op::StoreLocal(slot),
        ]);
        plan.iteration.step.push(Op::DropLocal(slot));
        plan.iteration.target = slot;
        plan.item = Local {
            slot,
            ty,
            mutable: false,
        };
        Ok(())
    }

    fn pipeline_arguments(&mut self, plan: &Pipeline, predicate: bool, at: &Token) -> Vec<Expr> {
        let fields = plan
            .pair
            .as_ref()
            .map_or_else(|| vec![plan.item.clone()], |p| p.to_vec());
        fields
            .into_iter()
            .map(|field| {
                let name = format!("<pipeline value {}>", field.slot);
                let borrowed = matches!(field.ty, Ty::Ref(..));
                let mutable_reference = matches!(field.ty, Ty::Ref(_, true));
                self.bind(name.clone(), field);
                let value = Expr {
                    at: at.clone(),
                    kind: Expression::Name(name),
                };
                if predicate && mutable_reference {
                    Expr {
                        at: at.clone(),
                        kind: Expression::Unary(
                            "&".into(),
                            Box::new(Expr {
                                at: at.clone(),
                                kind: Expression::Unary("*".into(), Box::new(value)),
                            }),
                        ),
                    }
                } else if predicate && !borrowed {
                    Expr {
                        at: at.clone(),
                        kind: Expression::Unary("&".into(), Box::new(value)),
                    }
                } else {
                    value
                }
            })
            .collect()
    }

    /// A zip arm must produce one *accepted* item before the other arm advances.
    /// Adapter continues belong to this pull loop, not to the outer zip loop.
    /// Pair-valued arms must be packed before calling this helper.
    fn pipeline_pull(&mut self, plan: &mut Pipeline, at: &Token) -> Result<()> {
        plan.split = None;
        let ready = self.slot(Ty::Bool, at)?;
        // The pending slot is initialized even on exhaustion. Extract the item
        // only in the outer body's successful path, just like generator next().
        // This internal option may carry a reference protected by the source's
        // loan; it never becomes a user-visible stored reference.
        let option = crate::sum::option(plan.item.ty.clone());
        let Ty::Enum(sum) = &option else {
            unreachable!()
        };
        let pending = self.slot(option.clone(), at)?;
        let absent = vec![
            Op::Enum(crate::sum::EnumOp::New(sum.clone(), 0)),
            Op::StoreLocal(pending),
        ];
        let mut body = std::mem::take(&mut plan.iteration.body);
        body.extend(self.pipeline_value_ops(plan));
        body.extend([
            Op::Enum(crate::sum::EnumOp::New(sum.clone(), 1)),
            Op::StoreLocal(pending),
        ]);
        body.extend([Op::PushBool(true), Op::StoreLocal(ready), Op::Break]);
        let mut condition = absent.clone();
        condition.extend([
            Op::PushBool(false),
            Op::StoreLocal(ready),
            Op::Loop {
                condition: std::mem::take(&mut plan.iteration.condition).into(),
                body: body.into(),
            },
            Op::LoadLocal(ready),
        ]);
        plan.iteration.condition = condition;
        plan.iteration.absent.extend(absent);
        plan.iteration.body = vec![
            Op::MoveLocal(pending),
            Op::Enum(crate::sum::EnumOp::Take(sum.clone(), 1, 0)),
            Op::StoreLocal(plan.item.slot),
        ];
        Ok(())
    }

    fn pipeline_pair_adapter(
        &mut self,
        mut left: Pipeline,
        method: &str,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Pipeline> {
        if (method == "enumerate" && args.len() > 1) || (method == "zip" && args.len() != 1) {
            return Err(at.error(if method == "zip" {
                "zip requires one other iterable"
            } else {
                "enumerate takes an optional i64 start"
            }));
        }
        self.pipeline_pack_pair(&mut left, at)?;
        let fields = if method == "enumerate" {
            let counter = self.slot(Ty::I64, at)?;
            let first = self.slot(Ty::Bool, at)?;
            if let Some(start) = args.first() {
                let start = match &start.kind {
                    Expression::NamedArgument(name, value) if name == "start" => value,
                    _ => start,
                };
                let actual = self.expr_expected(start, Some(Ty::I64), ops)?;
                self.same(actual, Some(Ty::I64), &start.at)?;
            } else {
                ops.push(Op::PushInt(Value::I64(0)));
            }
            ops.extend([
                Op::StoreLocal(counter),
                Op::PushBool(true),
                Op::StoreLocal(first),
            ]);
            let mut advance = Vec::new();
            self.mark(at, &mut advance);
            advance.extend([
                Op::LoadLocal(counter),
                Op::PushInt(Value::I64(1)),
                Op::Add,
                Op::StoreLocal(counter),
            ]);
            left.iteration.body.extend([
                Op::LoadLocal(first),
                branch(vec![Op::PushBool(false), Op::StoreLocal(first)], advance),
            ]);
            [
                Local {
                    slot: counter,
                    ty: Ty::I64,
                    mutable: false,
                },
                left.item.clone(),
            ]
        } else {
            let mut right = self.pipeline_source(&args[0], ops)?;
            self.pipeline_pack_pair(&mut right, at)?;
            self.pipeline_pull(&mut left, at)?;
            self.pipeline_pull(&mut right, at)?;
            let mut absent = right.iteration.absent.clone();
            absent.push(Op::PushBool(false));
            left.iteration
                .condition
                .push(branch(right.iteration.condition, absent));
            left.iteration.absent.extend(right.iteration.absent);
            left.iteration.body.extend(right.iteration.body);
            left.iteration.cleanup.extend(right.iteration.cleanup);
            left.iteration.step.extend(right.iteration.step);
            left.iteration.exact_len = None;
            left.resources.extend(right.resources);
            left.initialized.extend(right.initialized);
            left.empty.extend(right.empty);
            left.owned &= right.owned;
            left.loans.extend(right.loans);
            [left.item.clone(), right.item]
        };
        let types: Vec<_> = fields.iter().map(|f| f.ty.clone()).collect();
        if Ty::identity_names_exceed_limit(&types) || types.iter().any(|ty| ty.layout_depth() >= 64)
        {
            return Err(at.error("type nesting exceeds the implementation limit of 64"));
        }
        left.item.ty = crate::sum::tuple(types);
        left.pair = Some(fields);
        Ok(left)
    }

    /// One loop over both sides: a `second` flag selects which side's
    /// condition, body, and step run. Each side's item moves into shared
    /// slots, so the consumer's body is emitted once.
    fn pipeline_chain(
        &mut self,
        mut left: Pipeline,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Pipeline> {
        let [other] = args else {
            return Err(at.error("chain requires one other iterable"));
        };
        let second = self.slot(Ty::Bool, at)?;
        ops.extend([Op::PushBool(false), Op::StoreLocal(second)]);
        let mut right = self.pipeline_source(other, ops)?;
        if right.item.ty != left.item.ty {
            return Err(other.at.error(format!(
                "chain requires both sides to yield the same item type; the first yields {}, the second yields {}",
                type_name(Some(left.item.ty.clone())),
                type_name(Some(right.item.ty.clone())),
            )));
        }
        self.pipeline_split_pair(&mut left, at)?;
        self.pipeline_split_pair(&mut right, at)?;
        let fields = |plan: &Pipeline| {
            plan.pair
                .as_ref()
                .map_or_else(|| vec![plan.item.clone()], |pair| pair.to_vec())
        };
        let (left_fields, right_fields) = (fields(&left), fields(&right));
        let mut shared = Vec::new();
        let mut left_moves = Vec::new();
        let mut right_moves = Vec::new();
        let mut step = Vec::new();
        let mut chained_loans = Vec::new();
        for (l, r) in left_fields.iter().zip(&right_fields) {
            let slot = self.slot(l.ty.clone(), at)?;
            let load = |local: &Local| {
                if local.ty.affine() {
                    Op::MoveLocal(local.slot)
                } else {
                    Op::LoadLocal(local.slot)
                }
            };
            left_moves.extend([load(l), Op::StoreLocal(slot)]);
            right_moves.extend([load(r), Op::StoreLocal(slot)]);
            if l.ty.managed() {
                step.push(Op::DropLocal(slot));
            }
            // A borrowed item keeps both sources' loans live wherever it is used.
            let origins: Vec<usize> = [l.slot, r.slot]
                .iter()
                .filter_map(|slot| self.reference_locals.get(slot).copied())
                .collect();
            if let Some(&first) = origins.first() {
                // A reborrow of the first origin, not a loan of the shared
                // slot, so the slot can be reassigned for the next item.
                let mut marker = Vec::new();
                let (root, mutable) = (self.loans[first].root, self.loans[first].mutable);
                let loan = self.new_loan(root, mutable, Some(first), &mut marker);
                self.loans[loan].precise = false;
                for origin in origins.into_iter().skip(1) {
                    let inherited = self.loans[origin].dependencies.clone();
                    self.loans[loan].dependencies.push(origin);
                    self.loans[loan].dependencies.extend(inherited);
                }
                let Some(Op::Loan(fact)) = marker.last_mut() else {
                    unreachable!()
                };
                *fact = self.loans[loan].clone();
                ops.extend(marker);
                chained_loans.push(loan);
                self.reference_locals.insert(slot, loan);
            }
            shared.push(Local {
                slot,
                ty: l.ty.clone(),
                mutable: false,
            });
        }
        let switch = [Op::PushBool(true), Op::StoreLocal(second)];
        let mut left_ended = left.iteration.step.clone();
        left_ended.extend(switch.clone());
        // The flag is true here; a split first loop reads false and ends.
        left_ended.extend([
            Op::LoadLocal(second),
            branch(vec![Op::Continue], vec![Op::Break]),
        ]);
        let (l, r) = (left.iteration, right.iteration);
        let mut first_ended = r.absent.clone();
        first_ended.push(Op::PushBool(true));
        let mut first_exhausted = switch.to_vec();
        first_exhausted.extend([
            Op::LoadLocal(second),
            branch(r.condition.clone(), vec![Op::PushBool(false)]),
        ]);
        let mut first = l.condition;
        first.push(branch(first_ended, first_exhausted));
        let mut later = l.absent.clone();
        later.extend(r.condition);
        let mut left_body = redirect_breaks(l.body, &left_ended);
        left_body.extend(left_moves);
        let mut right_body = r.body;
        right_body.extend(right_moves);
        let mut absent = l.absent;
        absent.extend(r.absent);
        let mut cleanup = l.cleanup;
        cleanup.extend(r.cleanup);
        let mut steps = vec![Op::LoadLocal(second), branch(r.step, l.step)];
        steps.extend(step);
        // Saturate: a later `take` may still bound a sum above i64.MAX.
        let exact_len = l.exact_len.zip(r.exact_len).map(|(a, b)| {
            let mut fits = b.clone();
            fits.push(Op::PushInt(Value::I64(i64::MAX)));
            fits.extend(a.clone());
            fits.extend([Op::Sub, Op::Gt]);
            let mut sum = a;
            sum.extend(b);
            sum.push(Op::Add);
            fits.push(branch(vec![Op::PushInt(Value::I64(i64::MAX))], sum));
            fits
        });
        left.iteration = Iteration {
            exact_len,
            cleanup,
            condition: vec![Op::LoadLocal(second), branch(later, first)],
            absent,
            body: vec![Op::LoadLocal(second), branch(right_body, left_body)],
            step: steps,
            target: shared[0].slot,
        };
        left.item = if shared.len() == 2 {
            Local {
                slot: shared[0].slot,
                ty: left.item.ty.clone(),
                mutable: false,
            }
        } else {
            shared[0].clone()
        };
        left.pair = (shared.len() == 2).then(|| [shared[0].clone(), shared[1].clone()]);
        left.resources.extend(right.resources);
        left.initialized.extend(right.initialized);
        left.empty.extend(right.empty);
        left.split = Some(second);
        left.owned &= right.owned;
        left.loans.extend(right.loans);
        left.loans.extend(chained_loans);
        left.depth = left.depth.max(right.depth + 1);
        Ok(left)
    }

    /// One loop whose condition pulls outer items until the callback's
    /// source yields one. `active` says whether the inner source's state
    /// holds a value; between outer items those slots are empty.
    fn pipeline_flat_map(
        &mut self,
        mut outer: Pipeline,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Pipeline> {
        let [argument] = args else {
            return Err(at.error("flat_map requires one argument"));
        };
        let callback = self.pipeline_callback(argument, ops)?;
        outer.owned &= !self.callback_borrows(&callback);
        let active = self.slot(Ty::Bool, at)?;
        ops.extend([Op::PushBool(false), Op::StoreLocal(active)]);
        let items = self.pipeline_arguments(&outer, false, at);
        let not_a_source = || {
            argument.at.error(
                "flat_map callbacks must return an owned list, set, dict, str, bytes, range, or generator",
            )
        };
        let not_iterable = |ty: &Ty| {
            ty.contains_reference()
                || (ty.element().is_none() && super::byte_buffers::byte_buffer(ty).is_none())
        };
        let mut fetch = Vec::new();
        let call = match &callback {
            Callback::Value(callee) => {
                let Expression::Name(function) = &callee.kind else {
                    unreachable!()
                };
                let call = Expr {
                    at: argument.at.clone(),
                    kind: Expression::Call(function.clone(), items),
                };
                if self
                    .expression_type_hint(&call)
                    .is_some_and(|ty| not_iterable(&ty))
                {
                    return Err(not_a_source());
                }
                call
            }
            // The spliced body yields the inner source, which the inner
            // iteration then moves out of its slot like an owned local.
            Callback::Spliced(splice) => {
                let ty = self
                    .splice_call(splice, &items, &mut fetch)?
                    .ok_or_else(not_a_source)?;
                if not_iterable(&ty) {
                    return Err(not_a_source());
                }
                let slot = self.slot(ty.clone(), &argument.at)?;
                fetch.push(Op::StoreLocal(slot));
                let name = format!("<pipeline inner source {slot}>");
                self.bind(
                    name.clone(),
                    Local {
                        slot,
                        ty,
                        mutable: false,
                    },
                );
                Expr {
                    at: argument.at.clone(),
                    kind: Expression::Name(name),
                }
            }
        };
        let inner_start = self.parameters + self.locals.len();
        let inner = self.pipeline_raw_source(&call, &mut fetch)?;
        let inner_fields = inner
            .pair
            .as_ref()
            .map_or_else(|| vec![inner.item.clone()], |pair| pair.to_vec());
        if inner_fields
            .iter()
            .any(|field| field.ty.contains_reference())
            || !inner.owned
        {
            return Err(not_a_source());
        }
        // State the fetch initializes persists across items: it starts empty
        // and is emptied again once the inner source is exhausted.
        let state: Vec<u8> = inner
            .initialized
            .iter()
            .copied()
            .filter(|&slot| slot as usize >= inner_start)
            .collect();
        ops.extend(state.iter().copied().map(Op::EmptyLocal));
        outer.empty.extend(state.iter().copied());
        outer.split = None;
        let mut release = inner.iteration.cleanup.clone();
        for &slot in &state {
            if self.locals[slot as usize - self.parameters].managed() {
                release.push(Op::DropLocal(slot));
            }
            release.push(Op::EmptyLocal(slot));
        }
        release.extend([Op::PushBool(false), Op::StoreLocal(active)]);
        let ready = self.slot(Ty::Bool, at)?;
        let o = outer.iteration;
        let i = inner.iteration;
        let mut produced = i.condition;
        produced.push(branch(
            vec![Op::PushBool(true), Op::StoreLocal(ready), Op::Break],
            release,
        ));
        let mut pull = vec![Op::LoadLocal(active), branch(produced, vec![])];
        pull.extend(o.condition);
        pull.push(branch(vec![], vec![Op::Break]));
        pull.extend(o.body);
        pull.extend(fetch);
        pull.extend(o.step);
        pull.extend([Op::PushBool(true), Op::StoreLocal(active)]);
        // Inner condition state is initialized on every exit from the pull.
        let mut condition = i.absent.clone();
        condition.extend([
            Op::PushBool(false),
            Op::StoreLocal(ready),
            Op::Loop {
                condition: vec![Op::PushBool(true)].into(),
                body: pull.into(),
            },
            Op::LoadLocal(ready),
        ]);
        let mut absent = i.absent;
        absent.extend(o.absent);
        let mut cleanup = o.cleanup;
        if !i.cleanup.is_empty() {
            cleanup.extend([Op::LoadLocal(active), branch(i.cleanup, vec![])]);
        }
        outer.iteration = Iteration {
            exact_len: None,
            cleanup,
            condition,
            absent,
            body: i.body,
            step: i.step,
            target: i.target,
        };
        outer.item = inner.item;
        outer.pair = inner.pair;
        outer.resources.extend(inner.resources);
        outer.owned &= inner.owned;
        Ok(outer)
    }

    /// `filter_value(v)` evaluates `v` once at construction and lowers
    /// `item == v` into the loop, so it accepts exactly the operands `==`
    /// accepts and calls nothing. A borrowed item reads through its
    /// reference; for borrowed items `v` may itself be a shared `&T`, whose
    /// loan is held like a borrowed source's until the pipeline ends.
    fn pipeline_filter_value(
        &mut self,
        plan: &mut Pipeline,
        args: &[Expr],
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<()> {
        if plan.pair.is_some() {
            return Err(at.error(
                "filter_value does not accept pair items; use filter with a two-parameter predicate",
            ));
        }
        let (borrowed, target) = match &plan.item.ty {
            Ty::Ref(inner, _) => (true, (**inner).clone()),
            ty => (false, ty.clone()),
        };
        let mut loan = None;
        let value_ty = match args {
            [] => {
                if target != Ty::Bool {
                    return Err(at.error(format!(
                        "filter_value() keeps items equal to True, but this pipeline yields {}; pass the value to keep",
                        type_name(Some(plan.item.ty.clone()))
                    )));
                }
                ops.push(Op::PushBool(true));
                Ty::Bool
            }
            [argument] => {
                let value = match &argument.kind {
                    Expression::NamedArgument(name, value) if name == "value" => value,
                    Expression::NamedArgument(name, _) => {
                        return Err(argument
                            .at
                            .error(format!("filter_value has no parameter named `{name}`")));
                    }
                    _ => argument,
                };
                let shared = Ty::Ref(Arc::new(target.clone()), false);
                let by_reference = borrowed
                    && (matches!(&ungroup(value).kind, Expression::Unary(op, _) if op == "&")
                        || matches!(self.expression_type_hint(value), Some(Ty::Ref(..))));
                let expected = if by_reference {
                    shared.clone()
                } else {
                    target.clone()
                };
                let actual = self.expr_expected(value, Some(expected.clone()), ops)?;
                if actual.as_ref() != Some(&expected) {
                    let accepted = if borrowed {
                        format!("{} or {}", type_name(Some(target.clone())), shared)
                    } else {
                        type_name(Some(target.clone()))
                    };
                    return Err(value.at.error(format!(
                        "filter_value compares {} items with a {} value; it requires {accepted}",
                        type_name(Some(plan.item.ty.clone())),
                        type_name(actual),
                    )));
                }
                if by_reference {
                    plan.owned = false;
                    loan = Some(self.reference_origin(value, ops)?);
                }
                expected
            }
            _ => return Err(at.error("filter_value takes at most one value")),
        };
        let slot = self.slot(value_ty.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        if let Some(loan) = loan {
            self.bind_reference(slot, loan, false, ops);
        }
        let name = |slot: u8| Expr {
            at: at.clone(),
            kind: Expression::Name(format!("<pipeline value {slot}>")),
        };
        let deref = |e: Expr| Expr {
            at: at.clone(),
            kind: Expression::Unary("*".into(), Box::new(e)),
        };
        let value_is_reference = matches!(value_ty, Ty::Ref(..));
        // Named for diagnostics such as a conflicting read through it.
        let argument = "filter_value argument".to_string();
        self.bind(
            argument.clone(),
            Local {
                slot,
                ty: value_ty,
                mutable: false,
            },
        );
        self.bind(
            format!("<pipeline value {}>", plan.item.slot),
            plan.item.clone(),
        );
        let item = name(plan.item.slot);
        let item = if borrowed { deref(item) } else { item };
        let value = Expr {
            at: at.clone(),
            kind: Expression::Name(argument),
        };
        let value = if value_is_reference {
            deref(value)
        } else {
            value
        };
        let equal = Expr {
            at: at.clone(),
            kind: Expression::Binary("==".into(), Box::new(item), Box::new(value)),
        };
        let output = self.expr(&equal, &mut plan.iteration.body)?;
        self.same(output, Some(Ty::Bool), at)?;
        plan.iteration.exact_len = None;
        let mut rejected = plan.iteration.step.clone();
        rejected.push(Op::Continue);
        plan.iteration.body.push(branch(vec![], rejected));
        Ok(())
    }

    fn pipeline_value_ops(&self, plan: &Pipeline) -> Vec<Op> {
        let load = |local: &Local| {
            if local.ty.affine() {
                Op::MoveLocal(local.slot)
            } else {
                Op::LoadLocal(local.slot)
            }
        };
        if let Some(pair) = &plan.pair {
            let Ty::Enum(tuple) = &plan.item.ty else {
                unreachable!()
            };
            vec![
                load(&pair[0]),
                load(&pair[1]),
                Op::Enum(crate::sum::EnumOp::New(tuple.clone(), 0)),
            ]
        } else {
            vec![load(&plan.item)]
        }
    }

    fn pipeline_callback(&mut self, callback: &Expr, ops: &mut Vec<Op>) -> Result<Callback> {
        if let Expression::Name(n) = &ungroup(callback).kind
            && self.sigs.contains_key(n)
            && !self.names.contains_key(n)
        {
            return Ok(Callback::Value(callback.clone()));
        }
        let ty = if let Expression::Anonymous(function) = &ungroup(callback).kind {
            if let Some(body) = splice::spliceable(function, &self.names) {
                return self.splice_callback(function, body, &callback.at, ops);
            }
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
        Ok(Callback::Value(Expr {
            at: callback.at.clone(),
            kind: Expression::Name(name),
        }))
    }

    fn pipeline_call(
        &mut self,
        callback: &Callback,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let callback = match callback {
            Callback::Spliced(splice) => return self.splice_call(splice, args, ops),
            Callback::Value(callback) => callback,
        };
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
        let mut plan = self.pipeline(e, ops)?;
        if names.len() == 2
            && let Some(pair) = &plan.pair
        {
            if names[0] == names[1] && names[0] != "_" {
                return Err(e.at.error("duplicate unpacking binding"));
            }
            for (name, local) in names.iter().zip(pair) {
                if name != "_" {
                    self.bind(name.clone(), local.clone());
                }
            }
        } else if names.len() == 1 {
            if plan
                .pair
                .as_ref()
                .is_some_and(|pair| pair.iter().any(|field| matches!(field.ty, Ty::Ref(..))))
            {
                return Err(e
                    .at
                    .error("a borrowed pair pipeline requires two unpacked bindings"));
            }
            self.pipeline_pack_pair(&mut plan, &e.at)?;
            self.bind(names[0].clone(), plan.item.clone());
        } else {
            return Err(e.at.error("pipeline binding arity does not match item"));
        }
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
        if name == "generator" {
            if !args.is_empty() {
                return Err(base.at.error("generator takes no arguments"));
            }
            return self.pipeline_generator(base, ops).map(Some);
        }
        if matches!(name, "collect" | "try_collect") {
            return self.pipeline_collect(base, name, args, None, ops).map(Some);
        }
        if matches!(name, "sum" | "min" | "max") {
            return self.pipeline_arithmetic(base, name, args, ops).map(Some);
        }
        let start = self.locals.len();
        let saved = self.names.clone();
        let plan = self.pipeline_source(base, ops)?;
        let items = self.pipeline_arguments(&plan, false, &base.at);
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
                    &[Expr {
                        at: initial.at.clone(),
                        kind: Expression::Name(acc),
                    }]
                    .into_iter()
                    .chain(items.clone())
                    .collect::<Vec<_>>(),
                    &mut body,
                )?;
                self.same(actual, Some(ty.clone()), callback.at())?;
                body.push(Op::StoreLocal(slot));
                Some((ty, slot))
            }
            ("try_fold", [initial, callback]) => {
                let accumulator_ty = self.value(initial, ops)?;
                let accumulator = self.slot(accumulator_ty.clone(), &initial.at)?;
                ops.push(Op::StoreLocal(accumulator));
                let acc_name = format!("<pipeline accumulator {accumulator}>");
                self.bind(
                    acc_name.clone(),
                    Local {
                        slot: accumulator,
                        ty: accumulator_ty.clone(),
                        mutable: true,
                    },
                );
                let callback = self.pipeline_callback(callback, ops)?;
                let mut call = Vec::new();
                let result = self.pipeline_call(
                    &callback,
                    &[Expr {
                        at: initial.at.clone(),
                        kind: Expression::Name(acc_name),
                    }]
                    .into_iter()
                    .chain(items.clone())
                    .collect::<Vec<_>>(),
                    &mut call,
                )?;
                let Some(ty @ Ty::Enum(_)) = result else {
                    return Err(callback
                        .at()
                        .error("try_fold callbacks must return Result[Accumulator, Error]"));
                };
                let success = result_payload(&ty).ok_or_else(|| {
                    callback
                        .at()
                        .error("try_fold callbacks must return Result[Accumulator, Error]")
                })?;
                self.same(Some(success), Some(accumulator_ty.clone()), callback.at())?;
                let Ty::Enum(result_type) = &ty else {
                    unreachable!()
                };
                let slot = self.slot(ty.clone(), &base.at)?;
                ops.extend([
                    if accumulator_ty.affine() {
                        Op::MoveLocal(accumulator)
                    } else {
                        Op::LoadLocal(accumulator)
                    },
                    Op::Enum(crate::sum::EnumOp::New(result_type.clone(), 0)),
                    Op::StoreLocal(slot),
                ]);
                body.extend([
                    Op::MoveLocal(slot),
                    Op::Enum(crate::sum::EnumOp::Take(result_type.clone(), 0, 0)),
                    Op::StoreLocal(accumulator),
                ]);
                body.extend(call);
                body.extend([
                    Op::StoreLocal(slot),
                    Op::LoadLocal(slot),
                    Op::Enum(crate::sum::EnumOp::Tag(result_type.clone())),
                    Op::PushInt(Value::I64(1)),
                    Op::Eq,
                    branch(vec![Op::Break], vec![]),
                ]);
                Some((ty, slot))
            }
            ("try_for_each", [callback]) => {
                let callback = self.pipeline_callback(callback, ops)?;
                let result = self.pipeline_call(&callback, &items, &mut body)?;
                let Some(ty @ Ty::Enum(_)) = result else {
                    return Err(callback
                        .at()
                        .error("try_for_each callbacks must return Result[(), Error]"));
                };
                let success = result_payload(&ty).ok_or_else(|| {
                    callback
                        .at()
                        .error("try_for_each callbacks must return Result[(), Error]")
                })?;
                self.same(Some(success), Some(Ty::Unit), callback.at())?;
                let Ty::Enum(result_type) = &ty else {
                    unreachable!()
                };
                let slot = self.slot(ty.clone(), &base.at)?;
                ops.extend([
                    Op::PushUnit,
                    Op::Enum(crate::sum::EnumOp::New(result_type.clone(), 0)),
                    Op::StoreLocal(slot),
                ]);
                body.extend([
                    Op::StoreLocal(slot),
                    Op::LoadLocal(slot),
                    Op::Enum(crate::sum::EnumOp::Tag(result_type.clone())),
                    Op::PushInt(Value::I64(1)),
                    Op::Eq,
                    branch(vec![Op::Break], vec![]),
                ]);
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
                let predicate_items = self.pipeline_arguments(&plan, true, &base.at);
                let actual = self.pipeline_call(&callback, &predicate_items, &mut body)?;
                self.same(actual, Some(Ty::Bool), callback.at())?;
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
                    let mut found = self.pipeline_value_ops(&plan);
                    found.extend([
                        Op::Enum(crate::sum::EnumOp::New(option.clone(), 1)),
                        Op::StoreLocal(slot),
                        Op::Break,
                    ]);
                    body.push(branch(found, vec![]));
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
                    &[Expr {
                        at: base.at.clone(),
                        kind: Expression::Name(acc_name),
                    }]
                    .into_iter()
                    .chain(items.clone())
                    .collect::<Vec<_>>(),
                    &mut combine,
                )?;
                self.same(actual, Some(plan.item.ty.clone()), callback.at())?;
                body.push(branch(combine, self.pipeline_value_ops(&plan)));
                body.extend([
                    Op::Enum(crate::sum::EnumOp::New(option.clone(), 1)),
                    Op::StoreLocal(slot),
                ]);
                Some((ty, slot))
            }
            ("for_each", [callback]) => {
                let callback = self.pipeline_callback(callback, ops)?;
                let actual = self.pipeline_call(&callback, &items, &mut body)?;
                self.same(actual, None, callback.at())?;
                None
            }
            _ => {
                return Err(base.at.error(format!(
                    "unsupported pipeline terminal or arguments: {name}"
                )));
            }
        };
        body.extend(plan.iteration.step.clone());
        pipeline_loop(&plan, body, ops);
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

    /// `sum`, `min` and `max`. A borrowed item is read through its reference,
    /// as `*item`, so the result is a value like the `std.math` reductions.
    /// `min`/`max` agree with `std.math.min_all`/`max_all`: equal items keep
    /// the first, -0.0 equals 0.0, and the first NaN item is the result.
    fn pipeline_arithmetic(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if !args.is_empty() {
            return Err(base.at.error(format!("{name} takes no arguments")));
        }
        let start = self.locals.len();
        let saved = self.names.clone();
        let plan = self.pipeline_source(base, ops)?;
        let ty = match &plan.item.ty {
            Ty::Ref(inner, _) if plan.pair.is_none() => (**inner).clone(),
            ty => ty.clone(),
        };
        let accepted = if name == "sum" {
            ty.is_numeric()
        } else {
            ty.is_numeric() || ty == Ty::Str
        };
        if plan.pair.is_some() || !accepted {
            let required = if name == "sum" {
                "numeric items"
            } else {
                "numeric or str items"
            };
            return Err(base.at.error(format!(
                "{name} requires {required}, found {}; map the items first or use fold",
                type_name(Some(plan.item.ty.clone()))
            )));
        }
        let [item] = self
            .pipeline_arguments(&plan, false, &base.at)
            .try_into()
            .unwrap_or_else(|_| unreachable!());
        let value = if matches!(plan.item.ty, Ty::Ref(..)) {
            Expr {
                at: base.at.clone(),
                kind: Expression::Unary("*".into(), Box::new(item)),
            }
        } else {
            item
        };
        let mut body = plan.iteration.body.clone();
        let named = |this: &mut Self, slot: u8, label: &str| {
            let name = format!("<pipeline {label} {slot}>");
            this.bind(
                name.clone(),
                Local {
                    slot,
                    ty: ty.clone(),
                    mutable: true,
                },
            );
            Expr {
                at: base.at.clone(),
                kind: Expression::Name(name),
            }
        };
        let output = if name == "sum" {
            let total = self.slot(ty.clone(), &base.at)?;
            ops.extend([zero(&ty), Op::StoreLocal(total)]);
            let sum = Expr {
                at: base.at.clone(),
                kind: Expression::Binary(
                    "+".into(),
                    Box::new(named(self, total, "sum")),
                    Box::new(value),
                ),
            };
            let actual = self.expr(&sum, &mut body)?;
            self.same(actual, Some(ty.clone()), &base.at)?;
            body.push(Op::StoreLocal(total));
            body.extend(plan.iteration.step.clone());
            pipeline_loop(&plan, body, ops);
            ops.push(Op::LoadLocal(total));
            ty.clone()
        } else {
            // `best` is meaningful only once `seen` is set.
            let seen = self.slot(Ty::Bool, &base.at)?;
            let best = self.slot(ty.clone(), &base.at)?;
            let current = self.slot(ty.clone(), &base.at)?;
            ops.extend([
                Op::PushBool(false),
                Op::StoreLocal(seen),
                Op::EmptyLocal(best),
                Op::EmptyLocal(current),
            ]);
            let actual = self.expr(&value, &mut body)?;
            self.same(actual, Some(ty.clone()), &base.at)?;
            body.push(Op::StoreLocal(current));
            let candidate = named(self, current, "candidate");
            let held = named(self, best, "best");
            let binary = |op: &str, left: Expr, right: Expr| Expr {
                at: base.at.clone(),
                kind: Expression::Binary(op.into(), Box::new(left), Box::new(right)),
            };
            // Matches `std.math.min_all`/`max_all`. A float candidate replaces
            // `best` unless it is ordered at or past it, so a NaN candidate wins
            // and equal values, including -0.0 and 0.0, keep the first. Once
            // `best` is NaN it stays, keeping the first NaN item.
            let better = if ty.is_float() {
                let ordered = binary(
                    if name == "min" { ">=" } else { "<=" },
                    candidate,
                    held.clone(),
                );
                binary(
                    "and",
                    binary("==", held.clone(), held),
                    Expr {
                        at: base.at.clone(),
                        kind: Expression::Unary("not".into(), Box::new(ordered)),
                    },
                )
            } else {
                binary(if name == "min" { "<" } else { ">" }, candidate, held)
            };
            let mut choose = Vec::new();
            let actual = self.expr(&better, &mut choose)?;
            self.same(actual, Some(Ty::Bool), &base.at)?;
            choose.push(branch(
                vec![Op::LoadLocal(current), Op::StoreLocal(best)],
                vec![],
            ));
            body.extend([
                Op::LoadLocal(seen),
                branch(
                    choose,
                    vec![
                        Op::PushBool(true),
                        Op::StoreLocal(seen),
                        Op::LoadLocal(current),
                        Op::StoreLocal(best),
                    ],
                ),
            ]);
            body.extend(plan.iteration.step.clone());
            pipeline_loop(&plan, body, ops);
            let option = crate::sum::option(ty.clone());
            let Ty::Enum(sum) = &option else {
                unreachable!()
            };
            ops.extend([
                Op::LoadLocal(seen),
                branch(
                    vec![
                        Op::LoadLocal(best),
                        Op::Enum(crate::sum::EnumOp::New(sum.clone(), 1)),
                    ],
                    vec![Op::Enum(crate::sum::EnumOp::New(sum.clone(), 0))],
                ),
            ]);
            option
        };
        self.finish_pipeline(&plan, ops);
        self.cleanup(start, ops);
        self.names = saved;
        Ok(output)
    }

    pub(super) fn pipeline_collect(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        expected: Type,
        ops: &mut Vec<Op>,
    ) -> Result<Ty> {
        if !args.is_empty() {
            return Err(base.at.error(format!(
                "{name} takes no arguments; annotate the target collection"
            )));
        }
        let target = expected.as_ref().and_then(result_payload);
        let Some(target @ (Ty::List(_) | Ty::Set(_) | Ty::Dict(..))) = target else {
            return Err(base.at.error(format!(
                "{name} requires an expected list[T] or set[T] or dict[K, V] collection type"
            )));
        };
        let start = self.locals.len();
        let mut plan = self.pipeline_source(base, ops)?;
        self.pipeline_pack_pair(&mut plan, &base.at)?;
        let (item, item_error) = if name == "try_collect" {
            let Some(item) = result_payload(&plan.item.ty) else {
                return Err(base.at.error("try_collect requires Result items"));
            };
            let Ty::Enum(result) = &plan.item.ty else {
                unreachable!()
            };
            (item, Some(result.get().variants[1].fields[0].clone()))
        } else {
            (plan.item.ty.clone(), None)
        };
        let expected_item = match &target {
            Ty::Dict(k, v) => Some(crate::sum::tuple(vec![(**k).clone(), (**v).clone()])),
            _ => target.element(),
        };
        self.same(Some(item), expected_item, &base.at)?;
        let result =
            self.collect_iteration(plan.iteration.clone(), target, item_error, &base.at, ops)?;
        self.finish_pipeline(&plan, ops);
        self.cleanup(start, ops);
        Ok(result)
    }
}

/// Emit the consumer loop. A split `chain` runs one loop per side, so neither
/// tests the flag per item. The first loop ends with the flag set when its side
/// is exhausted; a consumer `break` leaves it clear and skips the second loop.
fn pipeline_loop(plan: &Pipeline, body: Vec<Op>, ops: &mut Vec<Op>) {
    let condition = &plan.iteration.condition;
    let Some(flag) = plan.split else {
        ops.push(Op::Loop {
            condition: condition.clone().into(),
            body: body.into(),
        });
        return;
    };
    let side = |value| Op::Loop {
        condition: specialize(condition, flag, value).into(),
        body: specialize(&body, flag, value).into(),
    };
    ops.push(side(false));
    ops.extend([Op::LoadLocal(flag), branch(vec![side(true)], vec![])]);
}

/// Resolve each `branch` on `flag` to its `value` arm.
fn specialize(ops: &[Op], flag: u8, value: bool) -> Vec<Op> {
    let nested = |ops: &[Op]| -> Arc<[Op]> { specialize(ops, flag, value).into() };
    let mut out = Vec::with_capacity(ops.len());
    let mut i = 0;
    while i < ops.len() {
        if let (Op::LoadLocal(slot), Some(Op::Match(arms))) = (&ops[i], ops.get(i + 1))
            && *slot == flag
            && let Some(arm) = arms.iter().find(|arm| arm.pattern == Pattern::Bool(value))
        {
            out.extend(specialize(&arm.body, flag, value));
            i += 2;
            continue;
        }
        out.push(match &ops[i] {
            Op::Match(arms) => Op::Match(
                arms.iter()
                    .map(|arm| MatchArm {
                        pattern: arm.pattern,
                        body: nested(&arm.body),
                    })
                    .collect(),
            ),
            Op::If {
                then,
                elifs,
                otherwise,
            } => Op::If {
                then: nested(then),
                elifs: elifs
                    .iter()
                    .map(|arm| crate::op::ElifArm {
                        condition: nested(&arm.condition),
                        body: nested(&arm.body),
                    })
                    .collect(),
                otherwise: nested(otherwise),
            },
            Op::Loop { condition, body } => Op::Loop {
                condition: nested(condition),
                body: nested(body),
            },
            op => op.clone(),
        });
        i += 1;
    }
    out
}

/// Replace each `break` of `ops` that targets the enclosing loop. Breaks in
/// nested loops belong to those loops.
fn redirect_breaks(ops: Vec<Op>, replacement: &[Op]) -> Vec<Op> {
    let nested = |ops: &[Op]| -> Arc<[Op]> { redirect_breaks(ops.to_vec(), replacement).into() };
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        match op {
            Op::Break => out.extend(replacement.iter().cloned()),
            Op::Match(arms) => out.push(Op::Match(
                arms.iter()
                    .map(|arm| MatchArm {
                        pattern: arm.pattern,
                        body: nested(&arm.body),
                    })
                    .collect(),
            )),
            Op::If {
                then,
                elifs,
                otherwise,
            } => out.push(Op::If {
                then: nested(&then),
                elifs: elifs
                    .iter()
                    .map(|arm| crate::op::ElifArm {
                        condition: nested(&arm.condition),
                        body: nested(&arm.body),
                    })
                    .collect(),
                otherwise: nested(&otherwise),
            }),
            op => out.push(op),
        }
    }
    out
}

/// The additive identity of a numeric type: `sum` of no items.
fn zero(ty: &Ty) -> Op {
    match ty {
        Ty::F32 | Ty::F64 => Op::PushFloat {
            bits: 0,
            ty: ty.clone(),
        },
        Ty::I8 => Op::PushInt(Value::I8(0)),
        Ty::I16 => Op::PushInt(Value::I16(0)),
        Ty::I32 => Op::PushInt(Value::I32(0)),
        Ty::U8 => Op::PushInt(Value::U8(0)),
        Ty::U16 => Op::PushInt(Value::U16(0)),
        Ty::U32 => Op::PushInt(Value::U32(0)),
        Ty::U64 => Op::PushInt(Value::U64(0)),
        _ => Op::PushInt(Value::I64(0)),
    }
}

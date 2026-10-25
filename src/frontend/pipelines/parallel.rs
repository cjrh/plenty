//! Parallel pipelines: `source.par(&pool).map(f).filter(p).terminal()`.
//!
//! The chain is lowered into ordinary code: the owned source is split into
//! contiguous chunks, one synthesized immutable worker closure runs the serial
//! pipeline over a chunk, `pool.map` or `pool.map_result` runs the workers, and
//! a helper template combines the per-chunk results in chunk order. The worker
//! and its captured callbacks therefore pass the same worker-eligibility checks
//! as a direct `pool.map` call. See the reference's parallel pipelines section.
use super::*;

const HELPERS: &str = include_str!("../parallel_pipelines.plenty");
const TERMINALS: &str = "collect, try_collect, count, any, all, sum, min, max, or reduce";

/// Hidden callback locals keep this prefix so `pipeline_callback` can call a
/// worker's captured callback in place instead of moving it out.
pub(super) const CALLBACK: &str = "__plenty_par_cb";

struct Step {
    name: String,
    callback: Expr,
}

enum Source {
    List(Ty),
    Range,
}

impl Lower<'_> {
    /// True when `e` is an adapter chain that contains `.par(...)` on a
    /// pipeline source.
    pub(in crate::frontend) fn has_par(&self, e: &Expr) -> bool {
        match &ungroup(e).kind {
            Expression::Method(base, name, _, _) if name == "par" => self.is_pipeline(base),
            Expression::Method(base, _, _, _) => self.has_par(base),
            _ => false,
        }
    }

    fn par_helper(&mut self, name: &str) -> Result<String> {
        let internal = format!("__plenty_par_{name}");
        if !self.generics.templates.contains_key(&internal) {
            let mut function = modules::generated_interface(HELPERS)?
                .functions
                .into_iter()
                .find(|f| f.name == name)
                .expect("parallel pipeline helper");
            function.name = internal.clone();
            self.generics.validate_template(&function, self.aliases)?;
            self.generics.retention.keep(&function.body);
            self.generics
                .templates
                .insert(internal.clone(), Arc::new(function));
        }
        Ok(internal)
    }

    fn par_hidden(
        &mut self,
        label: &str,
        ty: Ty,
        at: &Token,
        hidden: &mut Vec<String>,
        ops: &mut Vec<Op>,
    ) -> Result<Expr> {
        let slot = self.slot(ty.clone(), at)?;
        ops.push(Op::StoreLocal(slot));
        let name = format!("__plenty_par_{label}{slot}");
        self.bind(
            name.clone(),
            Local {
                slot,
                ty,
                mutable: false,
            },
        );
        hidden.push(name.clone());
        Ok(name_expr(at, &name))
    }

    /// Lower `base.name(args)` where `base` contains `.par(...)`.
    pub(in crate::frontend) fn parallel_pipeline(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let mut hidden = Vec::new();
        let result = self.parallel_pipeline_inner(base, name, args, &mut hidden, ops);
        for name in hidden {
            self.names.remove(&name);
        }
        result
    }

    fn parallel_pipeline_inner(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        hidden: &mut Vec<String>,
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        let at = base.at.clone();
        if name == "par" {
            return Err(at.error(format!(
                "a parallel pipeline must end in {TERMINALS}; it cannot be stored, looped over, or parallelized twice"
            )));
        }
        match name {
            "collect" | "try_collect" | "count" | "any" | "all" | "sum" | "min" | "max"
            | "reduce" => {}
            "fold" | "try_fold" => {
                return Err(at.error(format!(
                    "{name} is not available after .par: its callback has no associativity guarantee; use reduce with a named function"
                )));
            }
            "for_each" | "try_for_each" => {
                return Err(at.error(format!(
                    "{name} is not available after .par: callbacks would run concurrently in no fixed order; collect first and loop over the list"
                )));
            }
            "map" | "filter" => {
                return Err(at.error(format!("a parallel pipeline must end in {TERMINALS}")));
            }
            "enumerate" => {
                return Err(at.error(
                    "enumerate must come before .par: chunk offsets are fixed when the source is split",
                ));
            }
            "take" | "skip" | "take_while" | "filter_value" | "zip" | "chain" | "flat_map" => {
                return Err(at.error(format!(
                    "{name} cannot follow .par: only map and filter run in parallel"
                )));
            }
            _ => {
                return Err(at.error(format!(
                    "{name} is not available on a parallel pipeline; end it with {TERMINALS}"
                )));
            }
        }
        // Collect the steps between `.par(...)` and the terminal.
        let mut steps = Vec::new();
        let mut current = base;
        let (source, par_args) = loop {
            let Expression::Method(inner, step, step_args, _) = &ungroup(current).kind else {
                unreachable!("has_par found .par")
            };
            if step == "par" {
                break (inner.as_ref(), step_args);
            }
            match step.as_str() {
                "map" | "filter" => {
                    let [callback] = step_args.as_slice() else {
                        return Err(current.at.error(format!("{step} takes one callback")));
                    };
                    steps.push(Step {
                        name: step.clone(),
                        callback: callback.clone(),
                    });
                }
                "enumerate" => {
                    return Err(current.at.error(
                        "enumerate must come before .par: chunk offsets are fixed when the source is split",
                    ));
                }
                _ => {
                    return Err(current.at.error(format!(
                        "{step} cannot follow .par: only map and filter run in parallel"
                    )));
                }
            }
            current = inner;
        };
        steps.reverse();
        let par_at = current.at.clone();
        let (pool, chunk_count) = self.par_arguments(par_args, &par_at)?;
        // Source forms: iter(S), iter(S).enumerate(k), enumerate(S, k).
        let (items, start) = match &ungroup(source).kind {
            Expression::Call(f, a) if f == "iter" && a.len() == 1 => (&a[0], None),
            Expression::Call(f, a) if f == "enumerate" && (1..=2).contains(&a.len()) => {
                (&a[0], Some(a.get(1)))
            }
            Expression::Method(inner, m, a, _)
                if m == "enumerate"
                    && a.len() <= 1
                    && matches!(&ungroup(inner).kind, Expression::Call(f, b) if f == "iter" && b.len() == 1) =>
            {
                let Expression::Call(_, b) = &ungroup(inner).kind else {
                    unreachable!()
                };
                (&b[0], Some(a.first()))
            }
            _ => {
                return Err(source.at.error(
                    "a parallel pipeline starts from iter(items), iter(items).enumerate(), or enumerate(items): only enumerate may come before .par",
                ));
            }
        };
        if self.is_generator_source(items) {
            return Err(items
                .at
                .error("par cannot split a generator: its items exist only one at a time"));
        }
        let source_ty = self.value(items, ops)?;
        let kind = match &source_ty {
            Ty::List(element) => Source::List((**element).clone()),
            Ty::Range(element) if **element == Ty::I64 => Source::Range,
            Ty::Range(element) => {
                return Err(items.at.error(format!(
                    "par splits range[i64] sources, found range[{element}]; map the items after .par"
                )));
            }
            Ty::Ref(..) => {
                return Err(items.at.error(
                    "par needs an owned list or range: workers cannot borrow the caller's data until scoped jobs exist (B22); move or copy the list",
                ));
            }
            other => {
                return Err(items.at.error(format!(
                    "par splits an owned list or range[i64], found {}",
                    type_name(Some(other.clone()))
                )));
            }
        };
        let element = match &kind {
            Source::List(element) => element.clone(),
            Source::Range => Ty::I64,
        };
        // Hold the source while the remaining construction arguments run.
        let range_parts = if let Source::Range = kind {
            ops.push(Op::RangeParts(Ty::I64));
            let len = self.par_hidden("len", Ty::I64, &at, hidden, ops)?;
            let step = self.par_hidden("step", Ty::I64, &at, hidden, ops)?;
            let first = self.par_hidden("first", Ty::I64, &at, hidden, ops)?;
            Some([first, step, len])
        } else {
            None
        };
        let source_local = if range_parts.is_none() {
            Some(self.par_hidden("source", source_ty.clone(), &at, hidden, ops)?)
        } else {
            None
        };
        let enumerated = start.is_some();
        let offset = match start.flatten() {
            Some(k) => {
                let actual = self.expr_expected(k, Some(Ty::I64), ops)?;
                self.same(actual, Some(Ty::I64), &k.at)?;
                self.par_hidden("start", Ty::I64, &k.at, hidden, ops)?
            }
            None => Expr {
                at: at.clone(),
                kind: Expression::Number("0".into()),
            },
        };
        let mut item = if enumerated {
            crate::sum::tuple(vec![Ty::I64, element])
        } else {
            element
        };
        // Callbacks: named functions stay names; others are built here and
        // moved into the worker.
        let mut captures = Vec::new();
        let mut worker_steps = Vec::new();
        for step in &steps {
            let (callback, output) =
                self.par_callback(&step.callback, hidden, &mut captures, ops)?;
            if step.name == "map" {
                item = output.ok_or_else(|| {
                    step.callback
                        .at
                        .error("a parallel map callback must return a value")
                })?;
            }
            worker_steps.push((step.name.clone(), callback));
        }
        let predicate = match (name, args) {
            ("any" | "all", [predicate]) => {
                Some(self.par_callback(predicate, hidden, &mut captures, ops)?.0)
            }
            ("any" | "all", _) => return Err(at.error(format!("{name} takes one predicate"))),
            ("reduce", [_]) => None,
            ("reduce", _) => return Err(at.error("reduce takes one named function")),
            (_, []) => None,
            _ => return Err(at.error(format!("{name} takes no arguments"))),
        };
        let reducer = if name == "reduce" {
            match &ungroup(&args[0]).kind {
                Expression::Name(f) if self.sigs.contains_key(f) && !self.names.contains_key(f) => {
                    Some(args[0].clone())
                }
                _ => {
                    return Err(args[0].at.error(
                        "a parallel reduce takes a named function (T, T) -> T, the same as reduce_tree",
                    ));
                }
            }
        } else {
            None
        };
        // Chunk count, then the chunks themselves.
        let parts = match chunk_count {
            Some(count) => {
                let actual = self.expr_expected(&count, Some(Ty::I64), ops)?;
                self.same(actual, Some(Ty::I64), &count.at)?;
                let parts = self.par_hidden("parts", Ty::I64, &count.at, hidden, ops)?;
                // A non-positive runtime count stops like an out-of-range cast.
                let check = Expr {
                    at: count.at.clone(),
                    kind: Expression::Call(
                        "u64".into(),
                        vec![Expr {
                            at: count.at.clone(),
                            kind: Expression::Binary(
                                "-".into(),
                                Box::new(parts.clone()),
                                Box::new(Expr {
                                    at: count.at.clone(),
                                    kind: Expression::Number("1".into()),
                                }),
                            ),
                        }],
                    ),
                };
                self.value(&check, ops)?;
                ops.push(Op::Drop);
                parts
            }
            None => {
                let (_, loans) = self.observe(&pool_place(&pool), ops)?;
                ops.push(Op::Executor(crate::executor::ExecutorOp::Workers));
                Self::end_reads(loans, ops);
                self.par_hidden("parts", Ty::I64, &at, hidden, ops)?
            }
        };
        let (split, split_args, chunk_items) = match (&range_parts, &source_local) {
            (Some([first, step, len]), _) => (
                "split_range",
                vec![first.clone(), step.clone(), len.clone(), offset, parts],
                Ty::Range(Arc::new(Ty::I64)),
            ),
            (None, Some(source)) => (
                "split_list",
                vec![source.clone(), offset, parts],
                source_ty.clone(),
            ),
            (None, None) => unreachable!(),
        };
        let split = self.par_helper(split)?;
        let chunks_ty = self
            .generic_call(&split, None, &split_args, &at, ops)
            .map_err(|e| internal_error(e, &at))?
            .expect("split result");
        let chunks = self.par_hidden("chunks", chunks_ty, &at, hidden, ops)?;
        let chunk_ty = crate::sum::tuple(vec![Ty::I64, chunk_items]);
        // The worker: the serial pipeline over one chunk.
        let offset_name = "__plenty_par_offset";
        let items_name = "__plenty_par_items";
        let mut chain = if enumerated {
            call(
                &at,
                "enumerate",
                vec![name_expr(&at, items_name), name_expr(&at, offset_name)],
            )
        } else {
            call(&at, "iter", vec![name_expr(&at, items_name)])
        };
        for (step, callback) in worker_steps {
            chain = method(&at, chain, &step, vec![callback]);
        }
        let mut extra = Vec::new();
        let (output, template) = match name {
            "collect" | "reduce" => {
                chain = method(&at, chain, "collect", vec![]);
                (
                    crate::sum::result(Ty::List(Arc::new(item.clone())), crate::sum::alloc_error()),
                    "collect",
                )
            }
            "try_collect" => {
                let Some(value) = result_payload(&item) else {
                    return Err(at.error("try_collect requires Result items"));
                };
                let Ty::Enum(result) = &item else {
                    unreachable!()
                };
                let error = crate::sum::anonymous(vec![
                    result.get().variants[1].fields[0].clone(),
                    crate::sum::alloc_error(),
                ]);
                chain = method(&at, chain, "try_collect", vec![]);
                (
                    crate::sum::result(Ty::List(Arc::new(value)), error),
                    "try_collect",
                )
            }
            "count" => {
                chain = method(&at, chain, "count", vec![]);
                (Ty::I64, "count")
            }
            "any" | "all" => {
                chain = method(&at, chain, name, vec![predicate.expect("predicate")]);
                (Ty::Bool, name)
            }
            "sum" => {
                if !item.is_numeric() {
                    return Err(at.error(format!(
                        "sum requires numeric items, found {}; map the items first",
                        type_name(Some(item.clone()))
                    )));
                }
                if item.is_int() && item.int_range().is_some_and(|(min, _)| min < 0) {
                    // Exact prefix tracking: the combined result traps
                    // exactly when the serial running total would overflow.
                    let step = self.par_helper("signed_step")?;
                    let step = self.specialize(&step, vec![item.clone()], &at)?;
                    let zero = || Expr {
                        at: at.clone(),
                        kind: Expression::Number("0".into()),
                    };
                    let mut start: Vec<Expr> = (0..6).map(|_| zero()).collect();
                    start.push(Expr {
                        at: at.clone(),
                        kind: Expression::Bool(false),
                    });
                    let init = Expr {
                        at: at.clone(),
                        kind: Expression::Tuple(start),
                    };
                    chain = method(&at, chain, "fold", vec![init, name_expr(&at, &step)]);
                    extra.push(call(
                        &at,
                        &item.to_string(),
                        vec![Expr {
                            at: at.clone(),
                            kind: Expression::Number("1".into()),
                        }],
                    ));
                    let mut fields = vec![Ty::I64; 6];
                    fields.push(Ty::Bool);
                    (crate::sum::tuple(fields), "signed_sum")
                } else {
                    chain = method(&at, chain, "sum", vec![]);
                    (item.clone(), "sum")
                }
            }
            "min" | "max" => {
                if !(item.is_numeric() || item == Ty::Str) {
                    return Err(at.error(format!(
                        "{name} requires numeric or str items, found {}; map the items first",
                        type_name(Some(item.clone()))
                    )));
                }
                chain = method(&at, chain, name, vec![]);
                extra.push(Expr {
                    at: at.clone(),
                    kind: Expression::Bool(name == "min"),
                });
                (crate::sum::option(item.clone()), "extremum")
            }
            _ => unreachable!(),
        };
        let unpack_names = vec![
            if enumerated { offset_name } else { "_" }.to_string(),
            items_name.to_string(),
        ];
        let typed = |ty: Ty| TypeRef {
            concrete: Some(ty),
            at: at.clone(),
            name: None,
            args: vec![],
        };
        let worker = Function {
            once: false,
            captures: captures
                .iter()
                .map(|name: &String| Capture {
                    name: name.clone(),
                    mutable: false,
                    borrowed: false,
                })
                .collect(),
            foreign: None,
            export: None,
            name: "par_worker".into(),
            type_params: vec![],
            at: par_at.clone(),
            inputs: vec![("__plenty_par_chunk".into(), typed(chunk_ty))],
            defaults: vec![],
            output: typed(output),
            doc: String::new(),
            doc_at: None,
            body: vec![
                Stmt {
                    at: at.clone(),
                    kind: Statement::Unpack {
                        names: unpack_names,
                        mutable: false,
                        value: name_expr(&at, "__plenty_par_chunk"),
                    },
                },
                Stmt {
                    at: at.clone(),
                    kind: Statement::Expr(chain),
                },
            ],
        };
        let worker_ty = self.closure_value(&worker, &par_at, ops)?;
        let worker = self.par_hidden("worker", worker_ty, &at, hidden, ops)?;
        let template = self.par_helper(template)?;
        let mut call_args = vec![pool.clone(), chunks, worker];
        call_args.extend(extra);
        let ty = self
            .generic_call(&template, None, &call_args, &at, ops)
            .map_err(|e| internal_error(e, &at))?;
        if template.ends_with("signed_sum") {
            // `(total, 0)`, or `(T.MAX, 1)` after an overflow, added here so
            // the trap names the terminal as the serial sum does.
            let pair = self.par_hidden("pair", ty.expect("sum result"), &at, hidden, ops)?;
            let (left, right) = ("__plenty_par_left", "__plenty_par_right");
            let add = Expr {
                at: at.clone(),
                kind: Expression::Binary(
                    "+".into(),
                    Box::new(name_expr(&at, left)),
                    Box::new(name_expr(&at, right)),
                ),
            };
            let cases = vec![
                result_case(
                    &at,
                    "Ok",
                    "__plenty_par_sums",
                    vec![
                        Stmt {
                            at: at.clone(),
                            kind: Statement::Unpack {
                                names: vec![left.into(), right.into()],
                                mutable: false,
                                value: name_expr(&at, "__plenty_par_sums"),
                            },
                        },
                        expr_stmt(call(&at, "Ok", vec![add])),
                    ],
                ),
                result_case(
                    &at,
                    "Err",
                    "__plenty_par_failure",
                    vec![expr_stmt(call(
                        &at,
                        "Err",
                        vec![name_expr(&at, "__plenty_par_failure")],
                    ))],
                ),
            ];
            let expected = crate::sum::result(item, crate::sum::pool_map_error());
            return self.match_value(&at, &pair, &cases, Some(expected), ops);
        }
        let Some(reducer) = reducer else {
            return Ok(ty);
        };
        // reduce: the collected items go through the pool's fixed tree.
        let collected =
            self.par_hidden("collected", ty.expect("collect result"), &at, hidden, ops)?;
        let place = pool_place(&pool);
        let binding = "__plenty_par_reduced";
        let failure = "__plenty_par_failure";
        let reduce = method(
            &at,
            place,
            "reduce_tree",
            vec![reducer, name_expr(&at, binding)],
        );
        let cases = vec![
            result_case(&at, "Ok", binding, vec![expr_stmt(reduce)]),
            result_case(
                &at,
                "Err",
                failure,
                vec![expr_stmt(call(&at, "Err", vec![name_expr(&at, failure)]))],
            ),
        ];
        let expected = crate::sum::result(crate::sum::option(item), crate::sum::pool_map_error());
        self.match_value(&at, &collected, &cases, Some(expected), ops)
    }

    /// `.par(&pool)` or `.par(&pool, chunks=n)`.
    fn par_arguments(&mut self, args: &[Expr], at: &Token) -> Result<(Expr, Option<Expr>)> {
        let (pool, count) = match args {
            [pool] => (pool, None),
            [pool, count] => match &count.kind {
                Expression::NamedArgument(label, value) if label == "chunks" => {
                    (pool, Some((**value).clone()))
                }
                _ => {
                    return Err(count.at.error(
                        "par takes a borrowed pool and an optional chunks=N: .par(&pool, chunks=4)",
                    ));
                }
            },
            _ => {
                return Err(at.error(
                    "par takes a borrowed pool and an optional chunks=N: .par(&pool, chunks=4)",
                ));
            }
        };
        let pool_ty = match &ungroup(pool).kind {
            Expression::Unary(op, place) if op == "&" => self.place_type(place),
            Expression::Name(n) => self.names.get(n).map(|local| local.ty.clone()),
            _ => None,
        };
        let is_executor = |ty: &Ty| matches!(ty, Ty::Executor);
        let borrowed = match (&ungroup(pool).kind, &pool_ty) {
            (Expression::Unary(..), Some(ty)) => {
                is_executor(&classes::unbox(ty.clone()))
                    || matches!(ty, Ty::Ref(inner, _) if is_executor(inner))
            }
            (Expression::Name(_), Some(Ty::Ref(inner, _))) => is_executor(inner),
            _ => false,
        };
        if !borrowed {
            return Err(pool
                .at
                .error("par takes a borrowed ThreadPoolExecutor: .par(&pool)"));
        }
        if let Some(count) = &count
            && let Expression::Number(n) = &count.kind
            && n.parse::<i64>().is_ok_and(|n| n < 1)
        {
            return Err(count.at.error("chunks must be a positive i64"));
        }
        if let Some(count) = &count
            && let Expression::Unary(op, inner) = &count.kind
            && op == "-"
            && matches!(inner.kind, Expression::Number(_))
        {
            return Err(count.at.error("chunks must be a positive i64"));
        }
        Ok((pool.clone(), count))
    }

    /// A callback for the worker: a named function stays a name; anything
    /// else must be a reusable immutable closure owning its captures, built
    /// here and captured by the worker.
    fn par_callback(
        &mut self,
        callback: &Expr,
        hidden: &mut Vec<String>,
        captures: &mut Vec<String>,
        ops: &mut Vec<Op>,
    ) -> Result<(Expr, Type)> {
        if let Expression::Name(n) = &ungroup(callback).kind
            && let Some(sig) = self.sigs.get(n)
            && !self.names.contains_key(n)
        {
            let output = sig.outputs.first().cloned();
            return Ok((callback.clone(), output));
        }
        let ty = match &ungroup(callback).kind {
            Expression::Anonymous(function) => self.closure_value(function, &callback.at, ops)?,
            _ => self.value(callback, ops)?,
        };
        let closure = match &ty {
            Ty::Closure(c) => c.clone(),
            Ty::Ref(..) => {
                return Err(callback.at.error(
                    "a parallel callback must be owned by the pipeline; move the closure in, not a reference to it",
                ));
            }
            _ => {
                return Err(callback.at.error(
                    "parallel callbacks must be named functions or reusable immutable closures",
                ));
            }
        };
        if closure.once {
            return Err(callback
                .at
                .error("a one-shot closure cannot be a parallel callback: each chunk calls it"));
        }
        if closure.mutable || closure.writable.iter().any(|w| *w) {
            return Err(callback.at.error(
                "a parallel callback cannot have mutable captures: chunks run concurrently",
            ));
        }
        if closure.borrowed
            || closure
                .captures
                .iter()
                .any(|(_, t)| matches!(t, Ty::Ref(..)))
        {
            return Err(callback.at.error(
                "a parallel callback cannot capture borrows: workers outlive the caller's loans; capture owned values",
            ));
        }
        let output = closure.signature.output.clone();
        let local = self.par_hidden(
            CALLBACK.trim_start_matches("__plenty_par_"),
            ty,
            &callback.at,
            hidden,
            ops,
        )?;
        let Expression::Name(name) = &local.kind else {
            unreachable!()
        };
        captures.push(name.clone());
        Ok((local, output))
    }
}

/// The pool written in `.par(&pool)`, without the borrow.
fn pool_place(pool: &Expr) -> Expr {
    match &ungroup(pool).kind {
        Expression::Unary(op, place) if op == "&" => (**place).clone(),
        _ => pool.clone(),
    }
}

fn expr_stmt(e: Expr) -> Stmt {
    Stmt {
        at: e.at.clone(),
        kind: Statement::Expr(e),
    }
}

/// `case Ok(name):` or `case Err(name):` with `body`.
fn result_case(at: &Token, variant: &str, name: &str, body: Vec<Stmt>) -> enums::Case {
    enums::Case {
        at: at.clone(),
        pattern: patterns::Pat {
            at: at.clone(),
            kind: patterns::PatKind::Variant(
                None,
                variant.into(),
                Some(vec![patterns::Pat {
                    at: at.clone(),
                    kind: patterns::PatKind::Binding(name.into()),
                }]),
            ),
        },
        body,
    }
}

fn name_expr(at: &Token, name: &str) -> Expr {
    Expr {
        at: at.clone(),
        kind: Expression::Name(name.into()),
    }
}

fn call(at: &Token, name: &str, args: Vec<Expr>) -> Expr {
    Expr {
        at: at.clone(),
        kind: Expression::Call(name.into(), args),
    }
}

fn method(at: &Token, base: Expr, name: &str, args: Vec<Expr>) -> Expr {
    Expr {
        at: at.clone(),
        kind: Expression::Method(Box::new(base), name.into(), args, Pos::of(at)),
    }
}

fn internal_error(error: Box<dyn Error>, at: &Token) -> Box<dyn Error> {
    let text = error.to_string();
    if text.contains("__plenty_par_") {
        at.error(format!(
            "parallel pipeline: {}",
            text.replace("__plenty_par_", "parallel ")
        ))
    } else {
        error
    }
}

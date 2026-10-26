//! Closure literals written directly in a pipeline are lowered at their
//! consumer instead of as a hidden function: captures are evaluated once at
//! construction, and the body is lowered in place at each use with its
//! parameters bound to the item locals. A body that could observe the
//! difference keeps the closure call; see [`spliceable`].
use super::*;

/// A pipeline callback as the plan uses it.
pub(in crate::frontend) enum Callback {
    /// A named function, or a closure value stored in a plan slot.
    Value(Expr),
    Spliced(Arc<Splice>),
}

pub(in crate::frontend) struct Splice {
    at: Token,
    inputs: Vec<(String, Ty)>,
    output: Type,
    body: Expr,
    environment: Vec<Environment>,
}

/// One capture's construction-time state.
struct Environment {
    name: String,
    slot: u8,
    ty: Ty,
    kind: Capture,
}

#[derive(Clone, Copy, PartialEq)]
enum Capture {
    /// An immutable scalar snapshot, read directly so it can stay in a register.
    Scalar,
    /// An owned snapshot, lent to each evaluation as a closure body sees it.
    Owned { mutable: bool },
    /// A reference held from construction until the plan is consumed.
    Borrowed { loan: usize },
}

impl Callback {
    pub(in crate::frontend) fn at(&self) -> &Token {
        match self {
            Callback::Value(e) => &e.at,
            Callback::Spliced(splice) => &splice.at,
        }
    }
}

/// The body expression of a closure literal that can be lowered in place.
///
/// A `?` inside a callback returns from the callback, not the consumer, so
/// such bodies keep the call; so do one-shot closures, nested closure literals,
/// and bodies naming an enclosing local they do not capture, whose diagnostics
/// belong to the hidden function.
pub(super) fn spliceable<'a>(
    function: &'a Function,
    enclosing: &HashMap<String, Local>,
) -> Option<&'a Expr> {
    if function.once {
        return None;
    }
    let [
        Stmt {
            kind: Statement::Expr(body),
            ..
        },
    ] = function.body.as_slice()
    else {
        return None;
    };
    let mut bound: HashSet<String> = function.inputs.iter().map(|(n, _)| n.clone()).collect();
    bound.extend(function.captures.iter().map(|c| c.name.clone()));
    plain(body, &bound, enclosing).then_some(body)
}

fn plain(e: &Expr, bound: &HashSet<String>, enclosing: &HashMap<String, Local>) -> bool {
    let free = |name: &String| bound.contains(name) || !enclosing.contains_key(name);
    let all = |values: &[Expr]| values.iter().all(|v| plain(v, bound, enclosing));
    match &e.kind {
        Expression::Try(_)
        | Expression::Anonymous(_)
        | Expression::Match(..)
        | Expression::Ambiguous { .. }
        | Expression::ClassNew(_)
        | Expression::ClassReady(..) => false,
        Expression::Name(n) | Expression::GenericValue(n, _) => free(n),
        Expression::Call(n, args) | Expression::GenericCall(n, _, args) => free(n) && all(args),
        Expression::Tuple(values) | Expression::Constructor(_, values) => all(values),
        Expression::Format(format) => format
            .operands()
            .into_iter()
            .all(|v| plain(v, bound, enclosing)),
        Expression::Method(base, _, args, _)
        | Expression::GenericMethod(base, _, _, args, _)
        | Expression::Invoke(base, args) => plain(base, bound, enclosing) && all(args),
        Expression::NamedArgument(_, base)
        | Expression::Member(base, _, _)
        | Expression::Group(base)
        | Expression::Unary(_, base) => plain(base, bound, enclosing),
        Expression::Index(left, right) | Expression::Binary(_, left, right) => {
            plain(left, bound, enclosing) && plain(right, bound, enclosing)
        }
        Expression::Conditional { condition, yes, no } => {
            plain(condition, bound, enclosing)
                && plain(yes, bound, enclosing)
                && plain(no, bound, enclosing)
        }
        Expression::Collection {
            entries, clauses, ..
        } => {
            let mut inner = bound.clone();
            for clause in clauses {
                match clause {
                    Clause::For(bindings, iterable) => {
                        if !plain(iterable, &inner, enclosing) {
                            return false;
                        }
                        inner.extend(bindings.iter().cloned());
                    }
                    Clause::If(condition) => {
                        if !plain(condition, &inner, enclosing) {
                            return false;
                        }
                    }
                }
            }
            entries.iter().all(|(key, value)| {
                plain(key, &inner, enclosing)
                    && value.as_ref().is_none_or(|v| plain(v, &inner, enclosing))
            })
        }
        Expression::Type(_)
        | Expression::Number(_)
        | Expression::Text(_)
        | Expression::Bytes(_)
        | Expression::Bool(_)
        | Expression::Unit => true,
    }
}

fn owned_capture(ty: &Ty) -> bool {
    !ty.contains_reference() && !ty.contains_generator_frame()
}

impl Lower<'_> {
    /// Evaluate a spliceable closure literal's captures in order, with the
    /// checks and diagnostics of [`Self::closure_value`].
    pub(super) fn splice_callback(
        &mut self,
        function: &Function,
        body: &Expr,
        at: &Token,
        ops: &mut Vec<Op>,
    ) -> Result<Callback> {
        let mut environment = Vec::new();
        for capture in &function.captures {
            let source = Expr {
                at: at.clone(),
                kind: Expression::Name(capture.name.clone()),
            };
            if self.place_type(&source).as_ref().is_some_and(Ty::key_view) {
                return Err(at.error("dictionary key views cannot be captured; materialise keys_snapshot() or to_set()"));
            }
            if self
                .place_type(&source)
                .is_some_and(|ty| ty.carried_reference().is_some())
            {
                return Err(at.error(format!(
                    "`{}` holds a borrow and cannot be captured; match it first and pass the reference",
                    capture.name
                )));
            }
            let (ty, kind) = if capture.borrowed {
                let (ty, loan) = self.borrow(&source, capture.mutable, ops)?;
                (ty, Capture::Borrowed { loan })
            } else {
                let ty = self.value(&source, ops)?;
                if !owned_capture(&ty) {
                    return Err(at.error("this type cannot be captured yet"));
                }
                let scalar = !capture.mutable && (ty.is_numeric() || ty == Ty::Bool);
                let kind = if scalar {
                    Capture::Scalar
                } else {
                    Capture::Owned {
                        mutable: capture.mutable,
                    }
                };
                (ty, kind)
            };
            if ty.layout_depth() >= 64 {
                return Err(
                    at.error("closure capture nesting exceeds the implementation limit of 64")
                );
            }
            let slot = self.slot(ty.clone(), at)?;
            ops.push(Op::StoreLocal(slot));
            self.name_slot(slot, &capture.name);
            if let Capture::Borrowed { loan } = kind {
                self.bind_reference(slot, loan, false, ops);
            }
            environment.push(Environment {
                name: capture.name.clone(),
                slot,
                ty,
                kind,
            });
        }
        let inputs = function
            .inputs
            .iter()
            .map(|(name, t)| {
                Ok((
                    name.clone(),
                    t.resolve(self.aliases)?
                        .ok_or_else(|| at.error("unit parameters are not supported yet"))?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let output = function.output.resolve(self.aliases)?;
        callables::validate(
            &crate::op::CallableSig {
                inputs: inputs.iter().map(|(_, ty)| ty.clone()).collect(),
                output: output.clone(),
            },
            at,
        )?;
        if matches!(output, Some(Ty::Ref(..))) {
            return Err(
                at.error("capturing or one-shot closures cannot return references or yield yet")
            );
        }
        Ok(Callback::Spliced(Arc::new(Splice {
            at: at.clone(),
            inputs,
            output,
            body: body.clone(),
            environment,
        })))
    }

    /// Whether the callback's state holds references, which keeps the plan
    /// from being suspended in a generator.
    pub(super) fn callback_borrows(&self, callback: &Callback) -> bool {
        match callback {
            Callback::Value(e) => self
                .expression_type_hint(e)
                .is_some_and(|ty| ty.contains_reference()),
            Callback::Spliced(splice) => splice
                .environment
                .iter()
                .any(|env| matches!(env.kind, Capture::Borrowed { .. })),
        }
    }

    /// Lower one evaluation of a spliced body as a call of the closure would
    /// run it: arguments are passed as to the hidden function, and the body
    /// sees only its parameters and captures.
    pub(super) fn splice_call(
        &mut self,
        splice: &Splice,
        args: &[Expr],
        ops: &mut Vec<Op>,
    ) -> Result<Type> {
        if args.len() != splice.inputs.len() {
            return Err(splice.at.error(format!(
                "closure expects {} arguments, got {}",
                splice.inputs.len(),
                args.len()
            )));
        }
        let start = self.locals.len();
        let temporaries = self.expression_temps.len();
        let mut loans = self.call_arguments(args, &splice.inputs, ops)?;
        let mut slots = Vec::with_capacity(splice.inputs.len());
        for (_, ty) in &splice.inputs {
            slots.push(self.slot(ty.clone(), &splice.at)?);
        }
        ops.extend(slots.iter().rev().map(|&slot| Op::StoreLocal(slot)));
        let names = std::mem::take(&mut self.names);
        let captures = std::mem::take(&mut self.captures);
        let tail = std::mem::take(&mut self.source_tail);
        for ((name, ty), &slot) in splice.inputs.iter().zip(&slots) {
            self.bind(
                name.clone(),
                Local {
                    slot,
                    ty: ty.clone(),
                    mutable: false,
                },
            );
            if let Ty::Ref(_, mutable) = ty {
                let loan = self.new_loan(slot, *mutable, None, ops);
                self.reference_locals.insert(slot, loan);
            }
        }
        let mut held = Vec::new();
        for env in &splice.environment {
            let local = match env.kind {
                Capture::Scalar => Local {
                    slot: env.slot,
                    ty: env.ty.clone(),
                    mutable: false,
                },
                Capture::Borrowed { loan } => {
                    held.push(loan);
                    self.captures.insert(env.name.clone());
                    Local {
                        slot: env.slot,
                        ty: env.ty.clone(),
                        mutable: false,
                    }
                }
                Capture::Owned { mutable } => {
                    let ty = Ty::Ref(Arc::new(env.ty.clone()), mutable);
                    let slot = self.slot(ty.clone(), &splice.at)?;
                    loans.push(self.new_loan(env.slot, mutable, None, ops));
                    ops.extend([Op::BorrowLocal(env.slot, mutable), Op::StoreLocal(slot)]);
                    let loan = self.new_loan(slot, mutable, None, ops);
                    self.reference_locals.insert(slot, loan);
                    self.captures.insert(env.name.clone());
                    Local {
                        slot,
                        ty,
                        mutable: false,
                    }
                }
            };
            self.bind(env.name.clone(), local);
        }
        let result = self.expr_expected(&splice.body, splice.output.clone(), ops);
        self.names = names;
        self.captures = captures;
        self.source_tail = tail;
        let actual = result?;
        if actual
            .as_ref()
            .is_some_and(|t| !matches!(t, Ty::Ref(..)) && t.contains_reference())
        {
            return Err(splice
                .at
                .error("a closure with borrowed captures cannot escape its function"));
        }
        self.same(actual.clone(), splice.output.clone(), &splice.body.at)?;
        self.finish_temporaries(temporaries, ops);
        self.cleanup(start, ops);
        Self::end_reads(loans, ops);
        Self::end_reads(held, ops);
        Ok(match (&splice.output, actual) {
            (Some(expected), Some(actual)) => crate::generator::refine(expected, &actual),
            (output, _) => output.clone(),
        })
    }
}

//! Call-site substitution of tiny functions, before any body is lowered.
//!
//! - A *constant* function takes no parameters and returns one scalar
//!   literal. Every module `const` lowers to one (`$const.NAME`), and each
//!   read of the constant to a call of it. Calls to a constant become its
//!   literal, with the literal's own type, so the width, sign and float bits
//!   are those of the declaration. Constant getters are then unreferenced
//!   (the resolver lets a constant be used only as a value) and are removed.
//! - A *leaf predicate* takes scalar parameters, returns one scalar and
//!   computes it with comparisons, boolean operators and literal matches only:
//!   nothing in it can trap, call, allocate or touch memory. In optimized
//!   builds a call to one stores its arguments in fresh locals of the caller
//!   and runs the predicate's operations there. The definition is kept,
//!   since a function value or another object may still refer to it.
//!
//! Neither kind has an operation that reports a source position, so the
//! callee's position markers are dropped and the caller's stay in force: a
//! trap or a call after the substituted call reports exactly what it did
//! before. The caller's own `CallerSite` before the call is kept for the same
//! reason. The substituted operations are part of the caller's IR, so the
//! function cache keys on them; changing a constant or predicate changes every
//! caller's key.
use super::*;
use crate::op::CompiledFn;

/// Operations, not counting position and ownership markers, that a leaf
/// predicate may have. Bounds the code each call site grows by.
const PREDICATE_OPS: usize = 12;

/// Local slots are numbered by `u8`.
const SLOTS: usize = u8::MAX as usize + 1;

struct Callee {
    params: Vec<Ty>,
    /// Without markers, with constants substituted.
    body: Vec<Op>,
}

/// Substitute constants everywhere, and in optimized builds leaf predicates
/// in function bodies.
pub(super) fn substitute(ops: &mut Vec<Op>, optimize: bool, workers: usize) {
    let mut callees: HashMap<String, Callee> = HashMap::default();
    collect_constants(ops, &mut callees);
    if optimize {
        let mut predicates = Vec::new();
        collect_predicates(ops, &callees, &mut predicates);
        callees.extend(predicates);
    }
    if callees.is_empty() {
        return;
    }
    let callees = &callees;
    crate::parallel::map_mut(ops, workers, |op| {
        if let Some(new) = top_level(op, callees) {
            *op = new;
        }
    });
    ops.retain(|op| {
        !matches!(op, Op::DefineFn(name, _)
            if name.starts_with("$const.") && callees.get(name).is_some_and(|c| c.params.is_empty()))
    });
}

fn scalar(ty: &Ty) -> bool {
    ty.is_numeric() || *ty == Ty::Bool
}

fn literal(op: &Op) -> bool {
    matches!(op, Op::PushInt(_) | Op::PushFloat { .. } | Op::PushBool(_))
}

fn marker(op: &Op) -> bool {
    matches!(op, Op::Site(_) | Op::CallerSite(_) | Op::Access(..))
}

/// Every function defined in `ops`, nested definitions included.
fn definitions<'a>(ops: &'a [Op], visit: &mut impl FnMut(&'a str, &'a CompiledFn)) {
    for op in ops {
        if let Op::DefineFn(name, f) = op {
            visit(name, f);
            definitions(&f.body, visit);
        }
    }
}

fn collect_constants(ops: &[Op], callees: &mut HashMap<String, Callee>) {
    definitions(ops, &mut |name, f| {
        if !f.sig.inputs.is_empty()
            || f.generator.is_some()
            || !f.locals.is_empty()
            || !matches!(f.sig.outputs.as_slice(), [ty] if scalar(ty))
        {
            return;
        }
        let body: Vec<&Op> = f.body.iter().filter(|op| !marker(op)).collect();
        let value = match body.as_slice() {
            [value] if literal(value) => (*value).clone(),
            // `-f64.INF` and other negated float constants: `fneg` flips
            // only the sign bit, NaN payloads included.
            [Op::PushFloat { bits, ty }, Op::FloatNeg] => {
                let sign = if *ty == Ty::F32 { 1 << 31 } else { 1 << 63 };
                Op::PushFloat {
                    bits: bits ^ sign,
                    ty: ty.clone(),
                }
            }
            _ => return,
        };
        callees.insert(
            name.to_string(),
            Callee {
                params: vec![],
                body: vec![value],
            },
        );
    });
}

fn collect_predicates(
    ops: &[Op],
    constants: &HashMap<String, Callee>,
    out: &mut Vec<(String, Callee)>,
) {
    definitions(ops, &mut |name, f| {
        let params = f.sig.inputs.len();
        if params == 0
            || f.generator.is_some()
            || !f.locals.is_empty()
            || !f.sig.inputs.iter().all(|(_, ty)| scalar(ty))
            || !matches!(f.sig.outputs.as_slice(), [ty] if scalar(ty))
        {
            return;
        }
        let mut count = 0;
        if let Some(body) = pure(&f.body, params, constants, &mut count) {
            out.push((
                name.to_string(),
                Callee {
                    params: f.sig.inputs.iter().map(|(_, ty)| ty.clone()).collect(),
                    body,
                },
            ));
        }
    });
}

/// `ops` without markers and with constants substituted, when every
/// operation is one a leaf predicate may have.
fn pure(
    ops: &[Op],
    params: usize,
    constants: &HashMap<String, Callee>,
    count: &mut usize,
) -> Option<Vec<Op>> {
    let mut body = Vec::with_capacity(ops.len());
    for op in ops {
        if marker(op) {
            continue;
        }
        *count += 1;
        if *count > PREDICATE_OPS {
            return None;
        }
        body.push(match op {
            Op::LoadLocal(i) if (*i as usize) < params => op.clone(),
            Op::Call(name) => constants.get(name)?.body[0].clone(),
            Op::Match(arms) => {
                let mut rebuilt = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    if !matches!(
                        arm.pattern,
                        Pattern::Bool(_) | Pattern::Int { .. } | Pattern::Wildcard
                    ) {
                        return None;
                    }
                    rebuilt.push(MatchArm {
                        pattern: arm.pattern,
                        body: pure(&arm.body, params, constants, count)?.into(),
                    });
                }
                Op::Match(rebuilt.into())
            }
            Op::PushInt(_)
            | Op::PushFloat { .. }
            | Op::PushBool(_)
            | Op::Eq
            | Op::Ne
            | Op::Lt
            | Op::Gt
            | Op::Le
            | Op::Ge
            | Op::Not
            | Op::And
            | Op::Or => op.clone(),
            _ => return None,
        });
    }
    Some(body)
}

fn top_level(op: &Op, callees: &HashMap<String, Callee>) -> Option<Op> {
    match op {
        Op::DefineFn(name, f) => function(f, callees).map(|f| Op::DefineFn(name.clone(), f)),
        // `plenty_main`'s operations have no frame for predicate arguments.
        op => {
            let mut caller = Caller::new(callees, None);
            caller
                .body(std::slice::from_ref(op))
                .and_then(|mut ops| (ops.len() == 1).then(|| ops.pop().unwrap()))
        }
    }
}

fn function(f: &CompiledFn, callees: &HashMap<String, Callee>) -> Option<Box<CompiledFn>> {
    // A generator's locals live in its frame, whose layout is fixed by then.
    let slots = (f.generator.is_none()).then(|| f.sig.inputs.len() + f.locals.len());
    let mut caller = Caller::new(callees, slots);
    let body = caller.body(&f.body)?;
    let mut f = Box::new(f.clone());
    f.body = body.into();
    if !caller.added.is_empty() {
        f.locals = f.locals.iter().cloned().chain(caller.added).collect();
    }
    Some(f)
}

struct Caller<'a> {
    callees: &'a HashMap<String, Callee>,
    /// The next free local slot, or `None` when predicates are not
    /// substituted in this body.
    next: Option<usize>,
    /// Slots added for predicate arguments, after the existing locals.
    added: Vec<Ty>,
    /// Each predicate's first argument slot. Calls to one predicate share
    /// its slots: no argument outlives its call.
    bases: HashMap<&'a str, u8>,
}

impl<'a> Caller<'a> {
    fn new(callees: &'a HashMap<String, Callee>, next: Option<usize>) -> Self {
        Self {
            callees,
            next,
            added: Vec::new(),
            bases: HashMap::default(),
        }
    }

    /// `None` means that nothing in `ops` changes.
    fn body(&mut self, ops: &[Op]) -> Option<Vec<Op>> {
        let mut result: Option<Vec<Op>> = None;
        for (index, op) in ops.iter().enumerate() {
            let replaced = match op {
                Op::Call(name) => self.call(name, false),
                Op::TailCall(name) => self.call(name, true),
                _ => self.nested(op).map(|op| vec![op]),
            };
            match (replaced, &mut result) {
                (Some(new), Some(result)) => result.extend(new),
                (Some(new), None) => {
                    let mut started = ops[..index].to_vec();
                    started.extend(new);
                    result = Some(started);
                }
                (None, Some(result)) => result.push(op.clone()),
                (None, None) => {}
            }
        }
        result
    }

    fn shared(&mut self, ops: &Arc<[Op]>, changed: &mut bool) -> Arc<[Op]> {
        match self.body(ops) {
            Some(ops) => {
                *changed = true;
                ops.into()
            }
            None => Arc::clone(ops),
        }
    }

    fn nested(&mut self, op: &Op) -> Option<Op> {
        let mut changed = false;
        let op = match op {
            Op::Match(arms) => Op::Match(
                arms.iter()
                    .map(|arm| MatchArm {
                        pattern: arm.pattern,
                        body: self.shared(&arm.body, &mut changed),
                    })
                    .collect(),
            ),
            Op::Loop { condition, body } => Op::Loop {
                condition: self.shared(condition, &mut changed),
                body: self.shared(body, &mut changed),
            },
            Op::If {
                then,
                elifs,
                otherwise,
            } => Op::If {
                then: self.shared(then, &mut changed),
                elifs: elifs
                    .iter()
                    .map(|arm| ElifArm {
                        condition: self.shared(&arm.condition, &mut changed),
                        body: self.shared(&arm.body, &mut changed),
                    })
                    .collect(),
                otherwise: self.shared(otherwise, &mut changed),
            },
            Op::Try {
                source,
                target,
                cleanup,
            } => Op::Try {
                source: source.clone(),
                target: target.clone(),
                cleanup: self.shared(cleanup, &mut changed),
            },
            Op::DefineFn(name, f) => {
                return function(f, self.callees).map(|f| Op::DefineFn(name.clone(), f));
            }
            _ => return None,
        };
        changed.then_some(op)
    }

    fn call(&mut self, name: &str, tail: bool) -> Option<Vec<Op>> {
        let (name, callee) = self.callees.get_key_value(name)?;
        let mut ops = Vec::with_capacity(callee.body.len() + callee.params.len() + 1);
        if callee.params.is_empty() {
            ops.extend(callee.body.iter().cloned());
        } else {
            let base = self.base(name, callee)?;
            // Arguments are on the stack in call order, the last on top.
            ops.extend(
                (0..callee.params.len())
                    .rev()
                    .map(|i| Op::StoreLocal(base + i as u8)),
            );
            ops.extend(callee.body.iter().map(|op| relocate(op, base)));
        }
        // A tail call's results were the caller's; the stack holds nothing else.
        if tail {
            ops.push(Op::Return);
        }
        Some(ops)
    }

    fn base(&mut self, name: &'a str, callee: &Callee) -> Option<u8> {
        if let Some(&base) = self.bases.get(name) {
            return Some(base);
        }
        let next = self.next?;
        if next + callee.params.len() > SLOTS {
            return None;
        }
        let base = next as u8;
        self.added.extend(callee.params.iter().cloned());
        self.next = Some(next + callee.params.len());
        self.bases.insert(name, base);
        Some(base)
    }
}

/// `op` from a predicate body, with its parameter slots moved to `base`.
fn relocate(op: &Op, base: u8) -> Op {
    match op {
        Op::LoadLocal(i) => Op::LoadLocal(base + i),
        Op::Match(arms) => Op::Match(
            arms.iter()
                .map(|arm| MatchArm {
                    pattern: arm.pattern,
                    body: arm.body.iter().map(|op| relocate(op, base)).collect(),
                })
                .collect(),
        ),
        op => op.clone(),
    }
}

#[cfg(test)]
mod tests;

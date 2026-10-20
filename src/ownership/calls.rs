//! What a called function can wait for: the results of futures and pool
//! shutdowns it reaches, directly or through further calls (issue #349).
use crate::executor::ExecutorOp;
use crate::hash::{HashMap, HashSet};
use crate::op::{CompiledFn, Op, Ty};
use std::sync::OnceLock;

/// The functions of a program whose bodies are being checked. The call table
/// is built on first use: only a call that hands a traced future, or a pool
/// after a submission, to another function needs it.
pub struct Program<'p> {
    ops: &'p [Op],
    table: OnceLock<HashMap<&'p str, Body<'p>>>,
}

/// One function's own waits and its direct callees.
#[derive(Default)]
struct Body<'p> {
    /// Result types `T` of the `Future[T]` values whose `result()` it calls.
    results: Vec<Ty>,
    /// It calls `shutdown()` or owns a pool, which its drop shuts down.
    shuts: bool,
    /// It calls a value whose function is unknown.
    indirect: bool,
    callees: Vec<&'p str>,
}

/// The waits a call can reach, matched by type like the local checks.
#[derive(Default)]
pub(super) struct Effects {
    results: Vec<Ty>,
    shuts: bool,
    /// The callee, or a function it reaches, is not known.
    unknown: bool,
}

impl Effects {
    /// The call can wait for the result of a `Future[T]` it is handed.
    pub(super) fn waits(&self, result: &Ty) -> bool {
        self.unknown || self.results.contains(result)
    }

    /// The call can shut down a pool it is handed.
    pub(super) fn shuts(&self) -> bool {
        self.unknown || self.shuts
    }
}

impl<'p> Program<'p> {
    pub fn new(ops: &'p [Op]) -> Self {
        Self {
            ops,
            table: OnceLock::new(),
        }
    }

    fn table(&self) -> &HashMap<&'p str, Body<'p>> {
        self.table.get_or_init(|| {
            let mut table = HashMap::default();
            crate::threading::walk(self.ops, &mut |op| {
                if let Op::DefineFn(name, function) = op {
                    table.insert(name.as_str(), body(function));
                }
            });
            table
        })
    }

    /// Everything a call to `callee` can reach, or every effect for an
    /// indirect call (`None`). Recursion terminates through the visited set,
    /// which makes a cycle's effects the union of its members'.
    pub(super) fn effects(&self, callee: Option<&str>) -> Effects {
        let Some(callee) = callee else {
            return Effects {
                unknown: true,
                ..Effects::default()
            };
        };
        let table = self.table();
        let mut effects = Effects::default();
        let mut seen = HashSet::default();
        let mut pending = vec![callee];
        while let Some(name) = pending.pop() {
            if !seen.insert(name) {
                continue;
            }
            // A function compiled elsewhere has no body to inspect.
            let Some(body) = table.get(name) else {
                effects.unknown = true;
                break;
            };
            if body.indirect {
                effects.unknown = true;
                break;
            }
            effects.shuts |= body.shuts;
            for result in &body.results {
                if !effects.results.contains(result) {
                    effects.results.push(result.clone());
                }
            }
            pending.extend(body.callees.iter().copied());
        }
        effects
    }
}

fn body(function: &CompiledFn) -> Body<'_> {
    let mut body = Body {
        shuts: function
            .sig
            .inputs
            .iter()
            .map(|(_, ty)| ty)
            .chain(function.locals.iter())
            .any(crate::threading::owns_executor),
        ..Body::default()
    };
    crate::threading::walk(&function.body, &mut |op| match op {
        Op::Executor(ExecutorOp::Result(result)) => {
            if !body.results.contains(result) {
                body.results.push(result.clone());
            }
        }
        Op::Executor(ExecutorOp::Shutdown) => body.shuts = true,
        Op::Call(name) | Op::TailCall(name) => body.callees.push(name),
        Op::ClosureCall(closure) if !closure.name.is_empty() => body.callees.push(&closure.name),
        Op::ClosureCall(_)
        | Op::CallIndirect(_)
        | Op::TailCallIndirect(_)
        | Op::BoxedCallableCall(_) => body.indirect = true,
        _ => {}
    });
    body
}

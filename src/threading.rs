//! Closed-world worker eligibility and the compiler-owned scoped task contract.
use crate::hash::{HashMap, HashSet};
use crate::op::{CompiledFn, Op, Ty};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Task {
    pub adapter: String,
    pub worker: String,
    pub closure: Option<Arc<crate::closure::ClosureType>>,
    pub inputs: Vec<Ty>,
    pub output: Ty,
}

impl Task {
    pub fn owned_job(&self) -> Option<&Ty> {
        self.closure.as_ref()?;
        self.inputs.first().filter(|t| matches!(t, Ty::Closure(_)))
    }
    pub fn start_result(&self) -> Ty {
        crate::sum::result(
            Ty::Unit,
            self.owned_job()
                .map_or_else(crate::sum::thread_error, |job| {
                    crate::sum::spawn_error(job.clone())
                }),
        )
    }
    // pthread_t occupies the first word and the runtime's thread entry the
    // second. The result starts at the next aligned slot; copied inputs follow
    // it. None of these addresses may escape their scope.
    pub fn argument_offset(&self, index: usize) -> usize {
        16 + self.output.slot_bytes()
            + self.inputs[..index]
                .iter()
                .map(Ty::slot_bytes)
                .sum::<usize>()
    }
    pub fn bytes(&self) -> usize {
        self.argument_offset(self.inputs.len())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ThreadOp {
    Start(Arc<Task>, u8),
    Join(Arc<Task>),
    Finish(Arc<Task>, u8),
    /// std.actor: call the task's worker on this thread with an actor heap
    /// current. Pops the heap id below the inputs and pushes
    /// `Result[output, Trapped]`.
    Run(Arc<Task>),
    /// std.actor: pop an owned value without releasing it; its actor heap is
    /// discarded as a whole.
    Forget(Ty),
}

impl Task {
    /// The result of `__actor_run`.
    pub fn run_result(&self) -> Ty {
        crate::sum::result(self.output.clone(), crate::sum::trapped())
    }
}

/// Inspect each reachable body and stored type once. Cycles through recursive
/// source data and recursive functions terminate through their nominal identity.
/// A structural signature cannot certify the effects of an indirect callee.
pub fn check(ops: &[Op]) -> Result<(), Box<dyn std::error::Error>> {
    let mut roots = Vec::new();
    // Definitions are recorded during the one walk over every body; only a
    // program with roots indexes them by name.
    let mut definitions = Vec::with_capacity(ops.len());
    walk(ops, &mut |op| {
        if let Op::DefineFn(name, function) = op {
            definitions.push((name.as_str(), function.as_ref()));
        }
        if let Op::Thread(ThreadOp::Start(task, _)) = op {
            let mut types = task.inputs.clone();
            types.push(task.output.clone());
            roots.push(("worker", vec![task.worker.clone()], types));
        }
        if let Op::Channel(operation) = op {
            let (mut types, output) = operation.signature();
            types.push(output);
            roots.push(("channel message", vec![], types));
        }
        if let Op::Executor(operation) = op
            && let Some(job) = operation.job()
        {
            roots.push((
                "executor worker",
                vec![job.worker.clone()],
                std::iter::once(job.input.clone())
                    .chain(std::iter::once(job.output.clone()))
                    .chain(job.shared.iter().map(|t| Ty::Closure(t.clone())))
                    .collect(),
            ));
        }
    });
    // Only a value crossing a thread boundary starts an inspection. A program
    // without one needs no table of every function.
    if roots.is_empty() {
        return Ok(());
    }
    let functions: HashMap<_, _> = definitions.into_iter().collect();
    let mut checked = HashSet::default();
    let mut checked_receivers = HashSet::default();
    for (kind, mut queue, mut types) in roots {
        let origin = match queue.first() {
            Some(worker) => format!("{kind} {}", describe(worker, &functions)),
            None => kind.to_owned(),
        };
        let mut seen_types = HashSet::default();
        // The function that first queued each callee, to name the call path
        // to a rejected function: a generic instance or helper is otherwise
        // hard to trace back to the source that reached it.
        let mut callers: HashMap<String, String> = HashMap::default();
        let mut current: Option<String> = None;
        // The worker itself is named by `origin`; only its callees add context.
        let root = queue.first().cloned().unwrap_or_default();
        loop {
            let queued = queue.len();
            while let Some(ty) = types.pop() {
                check_type(
                    &ty,
                    &mut seen_types,
                    &mut checked_receivers,
                    &mut types,
                    &mut queue,
                )
                .map_err(|reason| {
                    match current.as_ref().filter(|name| *name != &root) {
                        // Pending types came from `current`; name it as inspect
                        // errors do, so the offending signature or local can be found.
                        Some(name) => format!(
                            "{origin} reaches {}{}: {reason}",
                            describe(name, &functions),
                            path(name, &callers, &functions)
                        ),
                        None => format!("{origin}: {reason}"),
                    }
                })?;
            }
            if let Some(caller) = &current {
                for callee in &queue[queued..] {
                    callers
                        .entry(callee.clone())
                        .or_insert_with(|| caller.clone());
                }
            }
            let Some(name) = queue.pop() else { break };
            if !checked.insert(name.clone()) {
                continue;
            }
            let function = functions
                .get(name.as_str())
                .ok_or_else(|| format!("{origin}: unavailable function `{name}`"))?;
            let queued = queue.len();
            inspect(function, &mut types, &mut queue).map_err(|reason| {
                format!(
                    "{origin} reaches {}{}: {reason}",
                    describe(&name, &functions),
                    path(&name, &callers, &functions)
                )
            })?;
            for callee in &queue[queued..] {
                callers
                    .entry(callee.clone())
                    .or_insert_with(|| name.clone());
            }
            current = Some(name);
        }
    }
    Ok(())
}

/// A function's name for a diagnostic. A generic instance's symbol is
/// internal, so it is named by the location of the definition it instantiates.
fn describe(name: &str, functions: &HashMap<&str, &CompiledFn>) -> String {
    if !name.starts_with("__plenty_generic_") {
        return format!("`{}`", crate::record::method_display(name));
    }
    match functions.get(name).and_then(|f| f.location.as_deref()) {
        Some(location) => format!("the generic function at {location}"),
        None => "a generic function".into(),
    }
}

/// The functions between the root and `name`, outermost first, as
/// ` through A -> B`; empty when the root reaches `name` directly.
fn path(
    name: &str,
    callers: &HashMap<String, String>,
    functions: &HashMap<&str, &CompiledFn>,
) -> String {
    let mut chain = Vec::new();
    let mut seen = HashSet::from_iter([name]);
    let mut at = name;
    while let Some(caller) = callers.get(at) {
        if !seen.insert(caller.as_str()) {
            break;
        }
        chain.push(caller.as_str());
        at = caller;
    }
    // The last caller is the root, which the origin already names.
    chain.pop();
    if chain.is_empty() {
        return String::new();
    }
    let names: Vec<_> = chain
        .iter()
        .rev()
        .map(|name| describe(name, functions))
        .collect();
    format!(" through {}", names.join(" -> "))
}

fn check_type(
    ty: &Ty,
    seen: &mut HashSet<String>,
    checked_receivers: &mut HashSet<String>,
    types: &mut Vec<Ty>,
    functions: &mut Vec<String>,
) -> Result<(), String> {
    match ty {
        Ty::Executor | Ty::Future(_) => {
            return Err("executor and future handles cannot enter worker threads; wait and submit on the owning thread".into())
        }
        Ty::Channel(message, false)
            if checked_receivers.insert(ty.identity_name()) && receiver_cycle(message, ty) =>
        {
            return Err(format!("recursive channel ownership through {ty} could retain its own receiver; keep receiving handles outside queued message cycles"));
        }
        // Native handles are affine owners whose mutating operations need an
        // exclusive loan, so a worker can own or borrow one like any value.
        // Their operations are certified per call in `inspect`.
        Ty::ForeignPtr(_) => {
            return Err(format!("{ty} has no cross-thread ownership contract"))
        }
        Ty::Callable(_) => {
            return Err("indirect callable effects are not certified for worker threads".into())
        }
        Ty::BoxedCallable(_) => {
            return Err("boxed callable captures and effects are not certified for worker threads".into())
        }
        Ty::Generator(_) => {
            return Err("generator worker eligibility is not implemented yet".into())
        }
        Ty::Ref(t, _)
        | Ty::Clist(t)
        | Ty::List(t)
        | Ty::Set(t)
        | Ty::Range(t)
        | Ty::Channel(t, _)
        | Ty::Box(t)
        // The last owner can drop on any holder's thread, so the content's
        // destructors are certified as if every holder could run them.
        | Ty::Shared(t) => types.push((**t).clone()),
        Ty::Dict(k, v) => types.extend([(**k).clone(), (**v).clone()]),
        Ty::Class(t) if seen.insert(format!("class:{}", t.name)) => {
            let t = t.get();
            types.extend(t.fields.iter().map(|(_, t)| t.clone()));
            functions.extend(t.destructor.iter().cloned());
        }
        Ty::Enum(t) if seen.insert(format!("enum:{}", t.name)) => {
            types.extend(
                t.get()
                    .variants
                    .iter()
                    .flat_map(|v| v.fields.iter().cloned()),
            );
        }
        Ty::Closure(t) if seen.insert(format!("closure:{}", t.name)) => {
            types.extend(t.captures.iter().map(|(_, t)| t.clone()));
            functions.push(t.name.clone());
        }
        // Task values are lexical tokens; their bodies are independently checked.
        Ty::Task(_) => {}
        _ => {}
    }
    Ok(())
}

/// Unlike a sender, the last receiver is responsible for releasing queued
/// values. A cycle back to that receiver would prevent this cleanup from ever
/// beginning. Inspect finite type identities, not potentially recursive values.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
fn receiver_cycle(message: &Ty, receiver: &Ty) -> bool {
    let mut pending = vec![message.clone()];
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        if &ty == receiver {
            return true;
        }
        if seen.insert(ty.clone()) {
            contained(&ty, &mut pending);
        }
    }
    false
}

/// The types a value of `ty` stores, queues, captures, or refers to.
pub fn contained(ty: &Ty, pending: &mut Vec<Ty>) {
    match ty {
        Ty::Channel(t, _)
        | Ty::Clist(t)
        | Ty::Future(t)
        | Ty::List(t)
        | Ty::Set(t)
        | Ty::Ref(t, _)
        | Ty::Box(t)
        | Ty::Shared(t) => pending.push((**t).clone()),
        Ty::Dict(k, v) => pending.extend([(**k).clone(), (**v).clone()]),
        Ty::Class(t) => pending.extend(t.get().fields.iter().map(|(_, t)| t.clone())),
        Ty::Enum(t) => pending.extend(
            t.get()
                .variants
                .iter()
                .flat_map(|v| v.fields.iter().cloned()),
        ),
        Ty::Closure(t) => pending.extend(t.captures.iter().map(|(_, t)| t.clone())),
        Ty::Generator(t) => pending.extend(t.slots.get().into_iter().flatten().cloned()),
        _ => {}
    }
}

/// Message types of the senders (or receivers) that a value of one of `roots`
/// can hold: directly, in aggregates, captures, and queued messages, and behind
/// references when `borrowed`. This bounds capabilities by type. It does not
/// identify queues, and it cannot see into an erased callable.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
pub fn endpoints(roots: impl IntoIterator<Item = Ty>, sender: bool, borrowed: bool) -> Vec<Ty> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut seen = HashSet::default();
    let mut found = Vec::new();
    while let Some(ty) = pending.pop() {
        if matches!(ty, Ty::Ref(..)) && !borrowed || !seen.insert(ty.clone()) {
            continue;
        }
        if let Ty::Channel(message, side) = &ty
            && *side == sender
            && !found.contains(&**message)
        {
            found.push((**message).clone());
        }
        contained(&ty, &mut pending);
    }
    found
}

/// Guarded types of the `Mutex[T]` values that `roots` can reach, owned or
/// borrowed. Like `endpoints`, this matches by type, not by identity.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
pub fn mutexes(roots: impl IntoIterator<Item = Ty>) -> Vec<Ty> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut seen = HashSet::default();
    let mut found = Vec::new();
    while let Some(ty) = pending.pop() {
        if !seen.insert(ty.clone()) {
            continue;
        }
        if let Some(class) = crate::mutex::class(&ty) {
            let element = crate::mutex::element(class);
            if !found.contains(&element) {
                found.push(element);
            }
        }
        contained(&ty, &mut pending);
    }
    found
}

/// A value of `ty` owns a pool, so its drop drains accepted work and joins the
/// workers. A reference leaves that to the pool's owner.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
pub fn owns_executor(ty: &Ty) -> bool {
    let mut pending = vec![ty.clone()];
    let mut seen = HashSet::default();
    while let Some(ty) = pending.pop() {
        match ty {
            Ty::Executor => return true,
            Ty::Ref(..) => continue,
            _ => {}
        }
        if seen.insert(ty.clone()) {
            contained(&ty, &mut pending);
        }
    }
    false
}

fn inspect(
    function: &CompiledFn,
    types: &mut Vec<Ty>,
    calls: &mut Vec<String>,
) -> Result<(), String> {
    types.extend(function.sig.inputs.iter().map(|(_, t)| t.clone()));
    types.extend(function.sig.outputs.iter().cloned());
    types.extend(function.locals.iter().cloned());
    let mut error = None;
    walk(&function.body, &mut |op| match op {
        Op::Call(name) | Op::TailCall(name) => calls.push(name.clone()),
        Op::ClosureNew(t) | Op::ClosureCall(t) => {
            // Concrete environments remain inspectable even when constructed
            // only as a temporary or returned inside a stored callback owner.
            types.push(Ty::Closure(t.clone()));
        }
        Op::ForeignCall { .. } => {
            error = Some("foreign calls have no worker-thread effect contract")
        }
        Op::CallIndirect(_) | Op::TailCallIndirect(_) => {
            error = Some("indirect callable effects are not certified for worker threads")
        }
        Op::Collection(op) => {
            // The runtime calls this key through the sort's adapter.
            if let crate::collection::CollectionOp::ListSortBy(sort) = op {
                calls.push(sort.key_of.clone());
            }
            if let Some(reason) = op.effect().worker_denial() {
                error = Some(reason);
            }
            let (inputs, output) = op.signature();
            types.extend(inputs);
            types.push(output);
        }
        Op::Channel(op) => {
            let (inputs, output) = op.signature();
            types.extend(inputs);
            types.push(output);
        }
        Op::Executor(op) => {
            let (inputs, output) = op.signature();
            types.extend(inputs);
            types.push(output);
        }
        Op::Control(op) => {
            let (inputs, output) = op.signature();
            types.extend(inputs);
            types.push(output);
        }
        // Atomic instructions are the shared-mutation contract for workers.
        Op::Atomic(op) => {
            let (inputs, output) = op.signature();
            types.extend(inputs);
            types.push(output);
        }
        // A lock makes its holder the value's only accessor.
        Op::Mutex(op) => {
            let (inputs, output) = op.signature();
            types.extend(inputs);
            types.push(output);
        }
        Op::Class(op) => {
            if let Some((inputs, output)) = op.signature() {
                types.extend(inputs);
                types.push(output);
            }
        }
        Op::Enum(op) => {
            if let Some((inputs, output)) = op.signature() {
                types.extend(inputs);
                types.push(output);
            }
        }
        Op::Box(op) => {
            let (inputs, output) = op.signature();
            types.extend(inputs);
            types.push(output);
        }
        Op::Split(t, tag) => {
            types.push(Ty::Enum(t.clone()));
            types.extend(t.get().variants[*tag].fields.iter().cloned());
        }
        Op::SplitClass(t) => {
            types.push(Ty::Class(t.clone()));
            types.extend(t.get().fields.iter().map(|(_, ty)| ty.clone()));
        }
        Op::ReadRef(ty) | Op::WriteRef(ty) | Op::FillRef(ty) | Op::ReplaceRef(ty) => {
            types.push(ty.clone())
        }
        _ => {}
    });
    error.map_or(Ok(()), |message| Err(message.into()))
}

/// What `find` returns for each operation in `ops`, nested ones included,
/// walking top-level operations on `workers` threads. Results keep the order
/// of the top-level operations they were found under.
pub fn find<'a, T: Send>(
    ops: &'a [Op],
    workers: usize,
    find: impl Fn(&'a Op) -> Option<T> + Sync,
) -> Vec<T> {
    crate::parallel::map(ops.len(), workers, |i| {
        let mut found = Vec::new();
        walk(std::slice::from_ref(&ops[i]), &mut |op| {
            found.extend(find(op))
        });
        found
    })
    .into_iter()
    .flatten()
    .collect()
}

pub fn walk<'a>(ops: &'a [Op], visit: &mut impl FnMut(&'a Op)) {
    let mut pending = vec![ops];
    while let Some(ops) = pending.pop() {
        for op in ops {
            visit(op);
            match op {
                Op::DefineFn(_, f) => pending.push(&f.body),
                Op::Match(arms) => pending.extend(arms.iter().map(|arm| arm.body.as_ref())),
                Op::Loop { condition, body } => pending.extend([condition.as_ref(), body.as_ref()]),
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    pending.extend([then.as_ref(), otherwise.as_ref()]);
                    for arm in elifs.iter() {
                        pending.extend([arm.condition.as_ref(), arm.body.as_ref()]);
                    }
                }
                Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => pending.push(cleanup),
                Op::CheckedBlock { body, .. } => pending.push(body),
                _ => {}
            }
        }
    }
}

/// `walk`, also passing the source position of each operation: the last
/// `Op::Site` before it in its own sequence or an enclosing one. A function
/// body starts without a position. `walk` stays separate because codegen's
/// whole-program scans do not need positions and tracking them cost 0.17% of
/// an AOT build's instructions.
pub fn walk_sited<'a>(
    ops: &'a [Op],
    visit: &mut impl FnMut(&'a Op, Option<crate::ownership::Site>),
) {
    let mut pending = vec![(ops, None)];
    while let Some((ops, mut site)) = pending.pop() {
        for op in ops {
            if let Op::Site(at) = op {
                site = Some(*at);
            }
            visit(op, site);
            match op {
                Op::DefineFn(_, f) => pending.push((&f.body, None)),
                Op::Match(arms) => {
                    pending.extend(arms.iter().map(|arm| (arm.body.as_ref(), site)));
                }
                Op::Loop { condition, body } => {
                    pending.extend([(condition.as_ref(), site), (body.as_ref(), site)]);
                }
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    pending.extend([(then.as_ref(), site), (otherwise.as_ref(), site)]);
                    for arm in elifs.iter() {
                        pending.extend([(arm.condition.as_ref(), site), (arm.body.as_ref(), site)]);
                    }
                }
                Op::Try { cleanup, .. } | Op::CheckedArith { cleanup, .. } => {
                    pending.push((cleanup, site))
                }
                Op::CheckedBlock { body, .. } => pending.push((body, site)),

                _ => {}
            }
        }
    }
}

/// Each function `op` names directly: calls, addresses, workers, closures,
/// sort keys, and the destructor an armed class runs. Calls that native code
/// derives from a type target generators only.
pub fn function_references(op: &Op, require: &mut impl FnMut(&str)) {
    match op {
        Op::Call(name)
        | Op::TailCall(name)
        | Op::FunctionAddress(name, _)
        | Op::BoxedCallableNew(_, name, _) => require(name),
        Op::ClosureNew(t) | Op::ClosureCall(t) => require(&t.name),
        Op::Thread(ThreadOp::Start(task, _) | ThreadOp::Run(task)) => {
            require(&task.worker);
            if let Some(closure) = &task.closure {
                require(&closure.name);
            }
        }
        Op::Executor(operation) => {
            if let Some(job) = operation.job() {
                require(&job.worker);
                for closure in job.closure.iter().chain(job.shared.iter()) {
                    require(&closure.name);
                }
            }
        }
        Op::Collection(crate::collection::CollectionOp::ListSortBy(sort)) => {
            require(&sort.key_of);
        }
        Op::Class(crate::record::ClassOp::ArmDrop(t)) => {
            if let Some(destructor) = &t.get().destructor {
                require(destructor);
            }
        }
        _ => {}
    }
}

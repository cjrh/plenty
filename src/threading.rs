//! Closed-world worker eligibility and the compiler-owned scoped task contract.
use crate::foreign::Threads;
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
    pub fn start_result(&self, drop_job: bool) -> Ty {
        let job = if drop_job {
            Ty::Unit
        } else {
            self.owned_job().cloned().unwrap_or(Ty::Unit)
        };
        crate::sum::result(Ty::Unit, crate::sum::spawn_error(job))
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
    /// Start with a job-free error when the direct call uses `drop_job()`.
    Start(Arc<Task>, u8, bool),
    Join(Arc<Task>),
    Finish(Arc<Task>, u8),
    /// std.actor: call the task's worker on this thread with an actor heap
    /// current. Pops the heap id below the inputs and pushes
    /// `Result[output, Trapped]`.
    Run(Arc<Task>),
    /// std.actor: pop an owned value without releasing it; its actor heap is
    /// discarded as a whole.
    Forget(Ty),
    /// A parallel pipeline's chunk jobs take items of this type from the
    /// caller's list in place, or borrow them when `true`. The chunks cross
    /// as integers, so this marker is the items' root for `check`; it emits
    /// no code.
    Rows(Ty, bool),
}

impl Task {
    /// The result of `__actor_run`.
    pub fn run_result(&self) -> Ty {
        crate::sum::result(self.output.clone(), crate::sum::trapped())
    }
}

/// Inspect each reachable body and stored type once. Cycles through recursive
/// source data and recursive functions terminate through their nominal identity.
/// Thin callable targets are bounded by the addresses in this closed program.
pub fn check(ops: &[Op]) -> Result<(), Box<dyn std::error::Error>> {
    let mut roots = Vec::new();
    // Definitions are recorded during the one walk over every body; only a
    // program with roots indexes them by name.
    let mut definitions = Vec::with_capacity(ops.len());
    let mut addresses = Vec::new();
    let mut jobs = Vec::new();
    walk(ops, &mut |op| {
        if let Op::DefineFn(name, function) = op {
            definitions.push((name.as_str(), function.as_ref()));
        }
        if let Op::FunctionAddress(name, signature) = op {
            addresses.push((name.as_str(), signature));
        }
        if let Op::Thread(ThreadOp::Start(task, _, _)) = op {
            let mut types = task.inputs.clone();
            types.push(task.output.clone());
            roots.push(("worker", vec![task.worker.clone()], moved(types)));
        }
        if let Op::Thread(ThreadOp::Rows(item, borrowed)) = op {
            let reach = if *borrowed { Reach::Lent } else { Reach::Moved };
            roots.push((
                "parallel pipeline items",
                vec![],
                vec![(item.clone(), reach)],
            ));
        }
        if let Op::Channel(operation) = op {
            let (mut types, output) = operation.signature();
            types.push(output);
            roots.push(("channel message", vec![], moved(types)));
        }
        if let Op::Executor(operation) = op
            && let Some(job) = operation.job()
        {
            jobs.push(job);
            roots.push((
                "executor worker",
                vec![job.worker.clone()],
                [
                    (job.input.clone(), Reach::Moved),
                    (job.output.clone(), Reach::Moved),
                ]
                .into_iter()
                // Every worker of the map calls one environment at once.
                .chain(
                    job.shared
                        .iter()
                        .map(|t| (Ty::Closure(t.clone()), Reach::Lent)),
                )
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
    let targets = CallableTargets::new(addresses);
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
            while let Some((ty, reach)) = types.pop() {
                check_type(
                    &ty,
                    reach,
                    &mut seen_types,
                    &mut checked_receivers,
                    &mut types,
                    &mut queue,
                    &targets,
                )
                .map_err(|reason| {
                    match current.as_ref().filter(|name| *name != &root) {
                        // Pending types came from `current`; name it as inspect
                        // errors do, so the offending signature or local can be found.
                        Some(name) => format!(
                            "{origin} reaches {}{}: {reason}",
                            describe(name, &functions),
                            path(name, &callers, &functions, &root)
                        ),
                        None => format!("{origin}: {reason}"),
                    }
                })?;
            }
            if let Some(caller) = current
                .as_deref()
                .or_else(|| (!root.is_empty()).then_some(root.as_str()))
            {
                for callee in &queue[queued..] {
                    callers
                        .entry(callee.clone())
                        .or_insert_with(|| caller.to_owned());
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
            let mut local = Vec::new();
            let inspected = inspect(function, &mut local, &mut queue);
            types.extend(local.into_iter().map(|ty| (ty, Reach::Local)));
            inspected.map_err(|reason| {
                format!(
                    "{origin} reaches {}{}: {reason}",
                    describe(&name, &functions),
                    path(&name, &callers, &functions, &root)
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
    crate::executor::submitters::check(&jobs, &functions, |name| describe(name, &functions))?;
    Ok(())
}

/// `FunctionAddress` is the sole source of thin callable values in typed IR.
/// FFI accepts only numeric/opaque scalars and their borrows, numeric casts
/// cannot reinterpret a callable, and storage only transfers existing values.
/// Boxed-call adapters construct native code addresses during codegen, but
/// their erased owner remains rejected separately. Thus every value of a
/// signature is bounded by *all* addresses of that signature in the program,
/// including addresses outside the worker's reachable call graph. Checking
/// only one apparently suitable target would make a conditional or forwarded
/// callback an effect escape. Signature-wide sets deliberately remain
/// conservative when unrelated main-thread code takes an unsafe address.
struct CallableTargets {
    by_signature: HashMap<String, Vec<String>>,
}

impl CallableTargets {
    fn new<'a>(
        addresses: impl IntoIterator<Item = (&'a str, &'a Arc<crate::op::CallableSig>)>,
    ) -> Self {
        let mut by_signature: HashMap<String, Vec<String>> = HashMap::default();
        for (name, signature) in addresses {
            by_signature
                .entry(Ty::Callable(signature.clone()).identity_name())
                .or_default()
                .push(name.to_owned());
        }
        for names in by_signature.values_mut() {
            names.sort_unstable();
            names.dedup();
        }
        Self { by_signature }
    }
}

/// A function's name for a diagnostic. A generic instance's symbol is
/// internal, so it is named by the location of the definition it instantiates.
fn describe(name: &str, functions: &HashMap<&str, &CompiledFn>) -> String {
    if !name.starts_with("__plenty_generic_") {
        return format!("`{}`", crate::record::method_display(name));
    }
    match functions.get(name).and_then(|f| f.location.as_ref()) {
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
    root: &str,
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
    // A named worker is already in the origin. Channel roots have no named
    // function, so their entire path remains useful context.
    if chain.last().copied() == Some(root) {
        chain.pop();
    }
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

/// How a value of a pending type reaches worker code. Only `Shareable`
/// opaque values may be copied, borrowed or `Shared` across threads; a
/// `Movable` one crosses only as a field of an owning class moved there,
/// which `copy(x)` must reject.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// Held by worker code itself: an inspected signature or local. Any value
    /// it received crossed through a root and was checked there.
    Local,
    /// Moved to the worker, or into a channel.
    Moved,
    /// A field of a moved class value that cannot be copied.
    Field,
    /// Copied, borrowed or shared, so another thread may still use it.
    Lent,
}

fn moved(types: Vec<Ty>) -> Vec<(Ty, Reach)> {
    types.into_iter().map(|ty| (ty, Reach::Moved)).collect()
}

fn check_type(
    ty: &Ty,
    reach: Reach,
    seen: &mut HashSet<String>,
    checked_receivers: &mut HashSet<String>,
    types: &mut Vec<(Ty, Reach)>,
    functions: &mut Vec<String>,
    targets: &CallableTargets,
) -> Result<(), String> {
    // Containers move their contents with them; borrows lend them.
    let within = match reach {
        Reach::Field => Reach::Moved,
        reach => reach,
    };
    let lent = match reach {
        Reach::Local => Reach::Local,
        _ => Reach::Lent,
    };
    match ty {
        Ty::Executor | Ty::Future(_) => {
            return Err("executor and future handles cannot enter worker threads; wait and submit on the owning thread, or give the worker a `pool.submitter()` to submit without waiting".into())
        }
        Ty::Channel(message, false)
            if checked_receivers.insert(ty.identity_name()) && receiver_cycle(message, ty) =>
        {
            return Err(format!("recursive channel ownership through {ty} could retain its own receiver; keep receiving handles outside queued message cycles"));
        }
        // The declaration's thread contract is the only fact about the C
        // resource; its operations are certified per call in `inspect`.
        Ty::ForeignPtr(_, threads) => match (threads, reach) {
            (Threads::None, _) => {
                return Err(format!("{ty} has no thread contract; worker code may hold an opaque value only if its .plentyi/.pi declaration is `@thread_movable` or `@thread_shareable`"))
            }
            (Threads::Movable, Reach::Moved | Reach::Lent) => {
                return Err(format!("{ty} is `@thread_movable`, not `@thread_shareable`: it may cross threads only as a field of a moved class value that `copy(x)` rejects, such as an owner with `__del__`; not copied, borrowed or `Shared`"))
            }
            _ => {}
        },
        Ty::Callable(signature) if seen.insert(format!("callable:{}", ty.identity_name())) => {
            let Some(names) = targets.by_signature.get(&ty.identity_name()) else {
                return Err(format!("indirect callable effects are not certified for worker threads: no statically known targets for {ty}"));
            };
            functions.extend(names.iter().cloned());
            // Arguments and results exist on the calling thread.
            types.extend(signature.inputs.iter().map(|t| (t.clone(), Reach::Local)));
            types.extend(signature.output.iter().map(|t| (t.clone(), Reach::Local)));
        }
        Ty::BoxedProtocol(_) => return Err("boxed protocol payloads and effects are not certified for worker threads".into()),
        Ty::BoxedCallable(_) => {
            return Err("boxed callable captures and effects are not certified for worker threads".into())
        }
        Ty::Generator(_) => {
            return Err("generator worker eligibility is not implemented yet".into())
        }
        Ty::Ref(t, _)
        | Ty::View(t, _)
        // The last owner can drop on any holder's thread, so the content's
        // destructors are certified as if every holder could run them.
        | Ty::Shared(t)
        // Local-only projections and iterator tokens cannot reach a worker
        // today; walked as borrows in case one ever does.
        | Ty::KeyView(t)
        | Ty::DictView(t, _)
        | Ty::Iter(t, _) => types.push(((**t).clone(), lent)),
        // Messages move through the queue whichever handle reached it.
        Ty::Channel(t, _) => types.push((
            (**t).clone(),
            if reach == Reach::Local { Reach::Local } else { Reach::Moved },
        )),
        Ty::Clist(t) | Ty::List(t) | Ty::Set(t) | Ty::Range(t) | Ty::Box(t) | Ty::Atomic(t) => {
            types.push(((**t).clone(), within))
        }
        Ty::Dict(k, v) => types.extend([((**k).clone(), within), ((**v).clone(), within)]),
        Ty::Class(c) if seen.insert(format!("class:{}:{reach:?}", c.name)) => {
            let t = c.get();
            let fields = match reach {
                Reach::Local => Reach::Local,
                // A lock makes its holder the value's only accessor.
                _ if crate::mutex::class(ty).is_some() => Reach::Moved,
                Reach::Lent => Reach::Lent,
                // A copy leaves the original fields behind. `copy(x)` accepts
                // any class without a destructor, not only `@copy` ones.
                _ if t.copy || ty.copy_facts().copyable => Reach::Lent,
                Reach::Moved | Reach::Field => Reach::Field,
            };
            types.extend(t.fields.iter().map(|(_, t)| (t.clone(), fields)));
            functions.extend(t.destructor.iter().cloned());
        }
        Ty::Enum(t) if seen.insert(format!("enum:{}:{reach:?}", t.name)) => {
            types.extend(
                t.get()
                    .variants
                    .iter()
                    .flat_map(|v| v.fields.iter().map(|t| (t.clone(), within))),
            );
        }
        Ty::Closure(t) if seen.insert(format!("closure:{}:{reach:?}", t.name)) => {
            types.extend(t.captures.iter().map(|(_, t)| (t.clone(), within)));
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
        | Ty::View(t, _)
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
        if matches!(ty, Ty::Ref(..) | Ty::View(..)) && !borrowed || !seen.insert(ty.clone()) {
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
            Ty::Ref(..) | Ty::View(..) => continue,
            _ => {}
        }
        if seen.insert(ty.clone()) {
            contained(&ty, &mut pending);
        }
    }
    false
}

/// Collect the types `function` stores, receives, or builds and the functions
/// it calls. An error names an operation without a worker-thread contract;
/// the walk still completes.
pub(crate) fn inspect(
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
        // The binding author's `@thread_safe` is the only effect contract a
        // C function can have; the compiler cannot inspect native code.
        Op::ForeignCall { declaration, .. } if !declaration.thread_safe => {
            error = Some(match declaration.symbol() {
                Some(symbol) => format!(
                    "C function `{symbol}` has no worker-thread effect contract; declare it `@thread_safe` in its .plentyi/.pi interface only if it may run on any thread"
                ),
                None => {
                    "foreign calls through a function address have no worker-thread effect contract"
                        .into()
                }
            })
        }
        Op::CallIndirect(signature) | Op::TailCallIndirect(signature) => {
            types.push(Ty::Callable(signature.clone()));
        }
        Op::Collection(op) => {
            // The runtime calls this key through the sort's adapter.
            if let crate::collection::CollectionOp::ListSortBy(sort) = op {
                calls.push(sort.key_of.clone());
            }
            // The runtime calls these formatting methods through adapters.
            if let crate::collection::CollectionOp::Formatted(formatted) = op {
                calls.extend(formatted.hooks.iter().map(|hook| hook.function.clone()));
            }
            if let Some(reason) = op.effect().worker_denial() {
                error = Some(reason.to_owned());
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
    error.map_or(Ok(()), Err)
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
                Op::Assumed(inner) => pending.push(std::slice::from_ref(inner)),
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
                Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                    pending.push(body)
                }
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
                Op::Assumed(inner) => pending.push((std::slice::from_ref(inner), site)),
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
                Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => {
                    pending.push((body, site))
                }

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
        Op::ProtocolBoxNew(_, adapters, _) => {
            for name in adapters {
                require(name);
            }
        }
        Op::ClosureNew(t) | Op::ClosureCall(t) => require(&t.name),
        Op::Thread(ThreadOp::Start(task, _, _) | ThreadOp::Run(task)) => {
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
        Op::Collection(crate::collection::CollectionOp::Formatted(formatted)) => {
            for hook in &formatted.hooks {
                require(&hook.function);
            }
        }
        Op::Class(crate::record::ClassOp::ArmDrop(t)) => {
            if let Some(destructor) = &t.get().destructor {
                require(destructor);
            }
        }
        _ => {}
    }
}

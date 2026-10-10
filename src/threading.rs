//! Closed-world worker eligibility and the compiler-owned scoped task contract.
use crate::op::{CompiledFn, Op, Ty};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Task {
    pub adapter: String,
    pub worker: String,
    pub closure: Option<Rc<crate::closure::ClosureType>>,
    pub inputs: Vec<Ty>,
    pub output: Ty,
}

impl Task {
    pub fn owned_job(&self) -> Option<&Ty> {
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
    // pthread_t occupies the first word. The result starts at an aligned slot;
    // copied inputs follow it. None of these addresses may escape their scope.
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
    Start(Rc<Task>, u8),
    Join(Rc<Task>),
    Finish(Rc<Task>, u8),
}

/// Inspect each reachable body and stored type once. Cycles through recursive
/// source data and recursive functions terminate through their nominal identity.
/// A structural signature cannot certify the effects of an indirect callee.
pub fn check(ops: &[Op]) -> Result<(), Box<dyn std::error::Error>> {
    let mut functions = HashMap::new();
    let mut roots = Vec::new();
    walk(ops, &mut |op| {
        if let Op::DefineFn(name, function) = op {
            functions.insert(name.as_str(), function);
        }
        if let Op::Thread(ThreadOp::Start(task, _)) = op {
            let mut types = task.inputs.clone();
            types.push(task.output.clone());
            roots.push((
                format!("worker `{}`", task.worker),
                vec![task.worker.clone()],
                types,
            ));
        }
        if let Op::Channel(operation) = op {
            let (mut types, output) = operation.signature();
            types.push(output);
            roots.push(("channel message".into(), vec![], types));
        }
        if let Op::Executor(operation) = op {
            if let Some(job) = operation.job() {
                roots.push((
                    format!("executor worker `{}`", job.worker),
                    vec![job.worker.clone()],
                    vec![job.input.clone(), job.output.clone()],
                ));
            }
        }
    });
    let mut checked = HashSet::new();
    let mut checked_receivers = HashSet::new();
    for (origin, mut queue, mut types) in roots {
        let mut seen_types = HashSet::new();
        loop {
            while let Some(ty) = types.pop() {
                check_type(
                    &ty,
                    &mut seen_types,
                    &mut checked_receivers,
                    &mut types,
                    &mut queue,
                )
                .map_err(|reason| format!("{origin}: {reason}"))?;
            }
            let Some(name) = queue.pop() else { break };
            if !checked.insert(name.clone()) {
                continue;
            }
            let function = functions
                .get(name.as_str())
                .ok_or_else(|| format!("{origin}: unavailable function `{name}`"))?;
            inspect(function, &mut types, &mut queue)
                .map_err(|reason| format!("{origin} reaches `{name}`: {reason}"))?;
        }
    }
    Ok(())
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
            if checked_receivers.insert(ty.to_string()) && receiver_cycle(message, ty) =>
        {
            return Err(format!("recursive channel ownership through {ty} could retain its own receiver; keep receiving handles outside queued message cycles"));
        }
        Ty::File | Ty::ForeignPtr(_) => {
            return Err(format!("{ty} has no cross-thread ownership contract"))
        }
        Ty::Callable(_) => {
            return Err("indirect callable effects are not certified for worker threads".into())
        }
        Ty::Generator(_) => {
            return Err("generator worker eligibility is not implemented yet".into())
        }
        Ty::Ref(t, _)
        | Ty::List(t)
        | Ty::Set(t)
        | Ty::Range(t)
        | Ty::Channel(t, _)
        | Ty::Box(t) => types.push((**t).clone()),
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
    let mut seen = HashSet::new();
    while let Some(ty) = pending.pop() {
        if &ty == receiver {
            return true;
        }
        if !seen.insert(ty.clone()) {
            continue;
        }
        match ty {
            Ty::Channel(t, _) | Ty::List(t) | Ty::Set(t) | Ty::Ref(t, _) | Ty::Box(t) => {
                pending.push((*t).clone())
            }
            Ty::Dict(k, v) => pending.extend([(*k).clone(), (*v).clone()]),
            Ty::Class(t) => pending.extend(t.get().fields.iter().map(|(_, t)| t.clone())),
            Ty::Enum(t) => pending.extend(
                t.get()
                    .variants
                    .iter()
                    .flat_map(|v| v.fields.iter().cloned()),
            ),
            Ty::Closure(t) => pending.extend(t.captures.iter().map(|(_, t)| t.clone())),
            _ => {}
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
        Op::ReadRef(ty) | Op::WriteRef(ty) | Op::ReplaceRef(ty) => types.push(ty.clone()),
        _ => {}
    });
    error.map_or(Ok(()), |message| Err(message.into()))
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
                Op::Try { cleanup, .. } => pending.push(cleanup),
                _ => {}
            }
        }
    }
}

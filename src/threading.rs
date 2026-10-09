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
    let mut tasks = Vec::new();
    walk(ops, &mut |op| {
        if let Op::DefineFn(name, function) = op {
            functions.insert(name.as_str(), function);
        }
        if let Op::Thread(ThreadOp::Start(task, _)) = op {
            tasks.push(task);
        }
    });
    let mut checked = HashSet::new();
    for task in tasks {
        let mut queue = vec![task.worker.clone()];
        let mut types = task.inputs.clone();
        types.push(task.output.clone());
        let mut seen_types = HashSet::new();
        loop {
            while let Some(ty) = types.pop() {
                check_type(&ty, &mut seen_types, &mut types, &mut queue)
                    .map_err(|reason| format!("worker `{}`: {reason}", task.worker))?;
            }
            let Some(name) = queue.pop() else { break };
            if !checked.insert(name.clone()) {
                continue;
            }
            let function = functions.get(name.as_str()).ok_or_else(|| {
                format!("worker `{}`: unavailable function `{name}`", task.worker)
            })?;
            inspect(function, &mut types, &mut queue)
                .map_err(|reason| format!("worker `{}` reaches `{name}`: {reason}", task.worker))?;
        }
    }
    Ok(())
}

fn check_type(
    ty: &Ty,
    seen: &mut HashSet<String>,
    types: &mut Vec<Ty>,
    functions: &mut Vec<String>,
) -> Result<(), String> {
    match ty {
        Ty::File | Ty::ForeignPtr(_) => {
            return Err(format!("{ty} has no cross-thread ownership contract"))
        }
        Ty::Callable(_) => {
            return Err("indirect callable effects are not certified for worker threads".into())
        }
        Ty::Generator(_) => {
            return Err("generator worker eligibility is not implemented yet".into())
        }
        Ty::Ref(t, _) | Ty::List(t) | Ty::Set(t) | Ty::Range(t) => types.push((**t).clone()),
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
        Op::ReadRef(ty) | Op::WriteRef(ty) => types.push(ty.clone()),
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

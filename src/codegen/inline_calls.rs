//! Constructor inlining during native lowering (issue #548). A class's
//! synthesized `new` and its `__init__` are small and called once per value
//! built, so the call, the caller-context bookkeeping around it and the copies
//! through the callee's frame dominate building a small object.
//!
//! The callee's typed body is lowered into the caller with its own locals,
//! source position, trap blocks and exits, so it keeps the semantics of the
//! call it replaces: arguments are already evaluated, a trap reports the
//! callee's position, and a `?` failure runs the callee's cleanup before
//! returning its error to the caller. Only the final `return` differs: it
//! jumps to a block in the caller instead.
//!
//! The compiled-function cache keys a function by its CLIF, which then
//! includes every inlined body, so a changed constructor changes its callers'
//! keys too.
use super::*;

/// Operations, excluding markers, in a callee and the callees inlined into it.
const MAX_OPERATIONS: usize = 64;

/// Whether `name` is a constructor: a class's `new` or its `__init__`.
fn constructor(name: &str) -> bool {
    crate::record::is_method(name) && (name.ends_with(".new") || name.ends_with(".__init__"))
}

fn marker(op: &Op) -> bool {
    matches!(
        op,
        Op::Site(_)
            | Op::CallerSite(_)
            | Op::Access(..)
            | Op::Loan(_)
            | Op::UseLoan(_)
            | Op::EndLoan(_)
            | Op::FieldMove(_)
            | Op::Returning(..)
            | Op::Building(_)
            | Op::Passes(_)
            | Op::Forwards(_)
            | Op::Tokens(_)
    )
}

enum State {
    Visiting,
    /// The operation count with inlined callees, or `None` when not inlined.
    Done(Option<usize>),
}

/// Mark the constructors to lower at their call sites: bounded in size and
/// not recursive. A real tail call releases the caller's locals before the
/// callee runs, which a jump back into the caller cannot express, so a body
/// with one stays a call.
pub(super) fn prepare(bodies: &[(String, Arc<[Op]>)], functions: &mut HashMap<String, UserFn>) {
    let candidates: HashMap<&str, &Arc<[Op]>> = bodies
        .iter()
        .filter(|(name, _)| constructor(name))
        .filter(|(name, _)| {
            let function = &functions[name];
            function.generator.is_none() && function.inline.is_none()
        })
        .map(|(name, body)| (name.as_str(), body))
        .collect();
    let mut states: HashMap<&str, State> = HashMap::default();
    // Declaration order keeps the outcome deterministic.
    for (name, _) in bodies {
        if candidates.contains_key(name.as_str()) {
            size(name, &candidates, &mut states);
        }
    }
    for (name, body) in &candidates {
        if let Some(State::Done(Some(_))) = states.get(name) {
            functions.get_mut(*name).unwrap().inline_body = Some(Arc::clone(body));
        }
    }
}

/// The operation count of `name` with its inlined callees, or `None` when it
/// stays a call. A callee reached again while its own count is pending is
/// recursive and stays a call; any cycle has such a callee, so inlining always
/// ends.
fn size<'a>(
    name: &'a str,
    candidates: &HashMap<&'a str, &'a Arc<[Op]>>,
    states: &mut HashMap<&'a str, State>,
) -> Option<usize> {
    match states.get(name) {
        Some(State::Done(size)) => return *size,
        Some(State::Visiting) => {
            states.insert(name, State::Done(None));
            return None;
        }
        None => {}
    }
    let body = *candidates.get(name)?;
    states.insert(name, State::Visiting);
    let mut ops = Vec::new();
    crate::threading::walk(body, &mut |op| ops.push(op));
    let mut total = Some(0usize);
    for op in ops {
        if marker(op) {
            continue;
        }
        let cost = match op {
            Op::TailCall(_) | Op::TailCallIndirect(_) | Op::Yield(_) | Op::DefineFn(..) => None,
            Op::Call(callee) if candidates.contains_key(callee.as_str()) => {
                Some(size(callee, candidates, states).unwrap_or(1))
            }
            _ => Some(1),
        };
        total = total.zip(cost).map(|(total, cost)| total + cost);
    }
    // A recursive call found below marked this function as staying a call.
    let size = match states.get(name) {
        Some(State::Done(None)) => None,
        _ => total.filter(|&total| total <= MAX_OPERATIONS),
    };
    states.insert(name, State::Done(size));
    size
}

impl Lowerer<'_, '_> {
    /// Lower a call of `name` by lowering its `body` here; `args` are the
    /// evaluated arguments in parameter order.
    pub(super) fn lower_inlined_call(
        &mut self,
        name: &str,
        body: &[Op],
        args: Vec<cranelift_codegen::ir::Value>,
        track_caller: bool,
    ) -> Result<()> {
        let functions = self.user_fns;
        let decl = &functions[name];
        let inputs = decl.sig.inputs.len();
        let saved = if track_caller {
            self.enter_call()?
        } else {
            None
        };
        let (empty_word, empty_double) = match self.empty_slot {
            Some(empty) => empty,
            None => {
                let word = self.bcx.ins().iconst(types::I64, 0);
                (word, self.bcx.ins().uextend(types::I128, word))
            }
        };
        let mut locals: Vec<(Variable, Ty)> = Vec::with_capacity(inputs + decl.locals.len());
        for ((_, ty), &arg) in decl.sig.inputs.iter().zip(&args) {
            let var = self.bcx.declare_var(clif_type(ty.clone()));
            self.bcx.def_var(var, arg);
            locals.push((var, ty.clone()));
        }
        for ty in decl.locals.iter() {
            let var = self.bcx.declare_var(clif_type(ty.clone()));
            // As at a function's entry: a managed slot starts empty, which
            // also resets it when the call site runs again in a loop.
            if ty.managed() {
                self.bcx
                    .def_var(var, if ty.wide() { empty_double } else { empty_word });
            }
            locals.push((var, ty.clone()));
        }
        let optimizes = self.module.optimizes();
        let portable_simd = portable_simd::State::new(body, &locals, inputs, self.bcx, optimizes);
        let mut addressed = addressed_locals(body, &locals);
        for (index, addressed) in addressed.iter_mut().enumerate() {
            if portable_simd.promoted(index as u8) {
                *addressed = false;
            }
        }
        let local_frame = local_frame(self.bcx, &locals, &addressed, inputs);
        let bytes: usize = decl.sig.outputs.iter().map(Ty::inline_bytes).sum();
        let return_storage = decl
            .sig
            .outputs
            .iter()
            .any(Ty::has_inline_storage)
            .then(|| self.inline_storage(bytes));
        let scalar_captures =
            scalar_captures::Captures::new(body, &locals, inputs, functions, self.bcx);
        let returned = self.bcx.create_block();
        let outputs = crate::tail_abi::layout(&decl.sig).outputs;
        for &ty in &outputs {
            self.bcx.append_block_param(returned, ty);
        }
        let srcloc = self.bcx.srcloc();
        let mut callee = Lowerer {
            bcx: &mut *self.bcx,
            module: &mut *self.module,
            runtime: self.runtime,
            user_fns: functions,
            str_data: self.str_data,
            eof_empty_str: self.eof_empty_str,
            locals: &locals,
            stack: Vec::new(),
            terminated: false,
            loop_targets: Vec::new(),
            generator: None,
            local_frame,
            return_storage,
            return_types: &decl.sig.outputs,
            collection_scratch: self.collection_scratch,
            fresh_load: None,
            scalar_captures,
            portable_simd,
            slot_copies: Vec::new(),
            empty_slot: Some((empty_word, empty_double)),
            exits: Some(exits::Exits::returning_to(returned)),
            counters: Default::default(),
            position: sites::Position::in_file(decl.source.clone(), decl.standard_library),
            trap_blocks: HashMap::default(),
            moves: Default::default(),
        };
        let lowered = callee.lower_body(body, inputs);
        self.collection_scratch = callee.collection_scratch;
        lowered.map_err(|e| -> Box<dyn Error> { format!("in inlined `{name}`: {e}").into() })?;
        self.bcx.switch_to_block(returned);
        self.bcx.seal_block(returned);
        self.bcx.set_srcloc(srcloc);
        self.leave_call(saved);
        let results = self.bcx.block_params(returned).to_vec();
        for (value, ty) in results.into_iter().zip(&decl.sig.outputs) {
            self.stack.push((value, ty.clone()));
        }
        Ok(())
    }

    /// Lower an inlined body as [`build_user_function`] lowers a function's.
    fn lower_body(&mut self, body: &[Op], inputs: usize) -> Result<()> {
        // Argument addresses belong to the caller: snapshot inline values.
        for (i, (_, ty)) in self.locals.iter().enumerate().take(inputs) {
            if ty.has_inline_storage() {
                let value = self.read_local(i as u8);
                self.write_local(i as u8, value);
            }
        }
        for op in body {
            if self.terminated {
                break;
            }
            self.lower(op)?;
        }
        if !self.terminated {
            self.leave(self.stack.clone(), &Arc::from([]))?;
        }
        self.finish_exits()?;
        self.seal_trap_blocks();
        self.finish_portable_simd();
        Ok(())
    }
}

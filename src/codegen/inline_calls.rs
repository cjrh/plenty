//! Call-site inlining during native lowering (issues #548 and #722). A
//! class's synthesized `new` and its `__init__` are small and called once per
//! value built, so the call, the caller-context bookkeeping around it and the
//! copies through the callee's frame dominate building a small object. Other
//! small functions gain the same, and a callee lowered in its caller also
//! shares the caller's facts: list headers, checked rows and induction
//! ranges.
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

/// Operations, excluding markers, in a constructor and the callees inlined
/// into it.
const MAX_CONSTRUCTOR_OPERATIONS: usize = 64;

/// The same bound for a function called from one place: its body is then
/// usually lowered only there (see [`Deferred`]), so the bound only limits
/// how large a caller grows.
const MAX_SINGLE_SITE_OPERATIONS: usize = 160;

/// The same bound for a function called from several places.
const MAX_SHARED_OPERATIONS: usize = 16;

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

struct Plan<'a> {
    candidates: HashMap<&'a str, &'a Arc<[Op]>>,
    /// Direct call sites of each function across every body.
    sites: HashMap<&'a str, usize>,
    states: HashMap<&'a str, State>,
}

/// Mark the functions to lower at their call sites in optimized builds:
/// bounded in size and not recursive (issues #548 and #722). The bound
/// depends on how often the body would be copied, so compile time grows
/// little: a constructor is small by construction, a function with one call
/// site is usually lowered only there, and one called from several places
/// must be about as small as the call it replaces.
///
/// A real tail call releases the caller's locals before the callee runs,
/// which a jump back into the caller cannot express, so a body with one
/// stays a call. So do generators and bodies that define functions.
pub(super) fn prepare(bodies: &[(String, Arc<[Op]>)], functions: &mut HashMap<String, UserFn>) {
    let candidates: HashMap<&str, &Arc<[Op]>> = bodies
        .iter()
        .filter(|(name, _)| {
            let function = &functions[name];
            function.generator.is_none() && function.inline.is_none()
        })
        .map(|(name, body)| (name.as_str(), body))
        .collect();
    let mut sites: HashMap<&str, usize> = HashMap::default();
    for (_, body) in bodies {
        crate::threading::walk(body, &mut |op| {
            if let Op::Call(callee) | Op::TailCall(callee) = op {
                *sites.entry(callee.as_str()).or_default() += 1;
            }
        });
    }
    let mut plan = Plan {
        candidates,
        sites,
        states: HashMap::default(),
    };
    // Declaration order keeps the outcome deterministic.
    for (name, _) in bodies {
        if plan.candidates.contains_key(name.as_str()) {
            plan.size(name);
        }
    }
    for (name, body) in &plan.candidates {
        if let Some(State::Done(Some(_))) = plan.states.get(name) {
            functions.get_mut(*name).unwrap().inline_body = Some(Arc::clone(body));
        }
    }
}

impl<'a> Plan<'a> {
    fn limit(&self, name: &str) -> usize {
        if constructor(name) {
            MAX_CONSTRUCTOR_OPERATIONS
        } else if self.sites.get(name) == Some(&1) {
            MAX_SINGLE_SITE_OPERATIONS
        } else {
            MAX_SHARED_OPERATIONS
        }
    }

    /// The operation count of `name` with its inlined callees, or `None` when
    /// it stays a call. A callee reached again while its own count is pending
    /// is recursive and stays a call; any cycle has such a callee, so inlining
    /// always ends.
    fn size(&mut self, name: &'a str) -> Option<usize> {
        match self.states.get(name) {
            Some(State::Done(size)) => return *size,
            Some(State::Visiting) => {
                self.states.insert(name, State::Done(None));
                return None;
            }
            None => {}
        }
        let body = *self.candidates.get(name)?;
        self.states.insert(name, State::Visiting);
        let mut ops = Vec::new();
        crate::threading::walk(body, &mut |op| ops.push(op));
        let mut total = Some(0usize);
        for op in ops {
            if marker(op) {
                continue;
            }
            let cost = match op {
                Op::TailCall(_) | Op::TailCallIndirect(_) | Op::Yield(_) | Op::DefineFn(..) => None,
                Op::Call(callee) if self.candidates.contains_key(callee.as_str()) => {
                    Some(self.size(callee).unwrap_or(1))
                }
                _ => Some(1),
            };
            total = total.zip(cost).map(|(total, cost)| total + cost);
        }
        // A recursive call found below marked this function as staying a call.
        let size = match self.states.get(name) {
            Some(State::Done(None)) => None,
            _ => total.filter(|&total| total <= self.limit(name)),
        };
        self.states.insert(name, State::Done(size));
        size
    }
}

impl Runtime {
    pub(super) fn refer(&self, functions: impl IntoIterator<Item = FuncId>) {
        self.referenced.lock().unwrap().extend(functions);
    }
}

/// Bodies of inlined functions, built only once placed code or data refers
/// to them: through a call the lowering kept, such as one in a generator, a
/// function value, a protocol table or a thread adapter. Otherwise the copy
/// at each call site is the function's only code, so inlining a function with
/// one call site costs no extra compile time.
pub(super) struct Deferred(Vec<(String, Arc<[Op]>)>);

impl Deferred {
    /// Move the deferred bodies out of `bodies`. Exported functions are
    /// referred to only after every body is placed, so they are built at once.
    pub(super) fn take(
        bodies: &mut Vec<(String, Arc<[Op]>)>,
        functions: &HashMap<String, UserFn>,
        exports: &[crate::exports::Export],
        enabled: bool,
    ) -> Self {
        if !enabled {
            return Self(Vec::new());
        }
        let exported: crate::hash::HashSet<&str> =
            exports.iter().map(|e| e.function.as_str()).collect();
        let (deferred, built) = std::mem::take(bodies).into_iter().partition(|(name, _)| {
            functions[name].inline_body.is_some() && !exported.contains(name.as_str())
        });
        *bodies = built;
        Self(deferred)
    }

    /// Build the deferred bodies something refers to, until a round adds no
    /// reference. The rest are reached only by inlined calls, but their
    /// symbols stay in the object, where a debugger or C code relinking it
    /// can still find them: each is defined as a single trap instruction, so
    /// such a call stops with `SIGILL` instead of running into the next
    /// function. Leaving them undefined would leave a local symbol with no
    /// definition, which object formats and linkers treat differently.
    pub(super) fn finish(
        mut self,
        out: &mut Output,
        workers: usize,
        runtime: &Runtime,
        functions: &HashMap<String, UserFn>,
        build: &(impl Fn(Job, &mut Worker) -> Result<()> + Sync),
    ) -> Result<()> {
        loop {
            let referenced = runtime.referenced.lock().unwrap().clone();
            let (now, later): (Vec<_>, Vec<_>) =
                std::mem::take(&mut self.0)
                    .into_iter()
                    .partition(|(name, _)| {
                        symbols(&functions[name]).any(|id| referenced.contains(&id))
                    });
            self.0 = later;
            if now.is_empty() {
                break;
            }
            let jobs = now
                .into_iter()
                .map(|(name, body)| Job::Function(name, body))
                .collect();
            out.build(jobs, workers, runtime, build)?;
        }
        if self.0.is_empty() {
            return Ok(());
        }
        let (alignment, trap) = trap_stub(out.module.isa())?;
        for (name, _) in &self.0 {
            for id in symbols(&functions[name]) {
                out.module
                    .define_function_bytes(id, alignment, &trap, &[])?;
            }
        }
        Ok(())
    }
}

/// The code of a function that only traps, as `Op::Unreachable` does: no
/// frame, no relocations and no check table entry, so its bytes serve any
/// signature. Compiled once per object rather than spelled per target.
fn trap_stub(isa: &dyn cranelift_codegen::isa::TargetIsa) -> Result<(u64, Vec<u8>)> {
    use cranelift_codegen::ir::{Function, Signature, UserFuncName};
    let mut context = cranelift_codegen::Context::for_function(Function::with_name_signature(
        UserFuncName::default(),
        Signature::new(isa.default_call_conv()),
    ));
    let mut builder_context = cranelift_frontend::FunctionBuilderContext::new();
    let mut bcx = cranelift_frontend::FunctionBuilder::new(&mut context.func, &mut builder_context);
    let block = bcx.create_block();
    bcx.switch_to_block(block);
    bcx.seal_block(block);
    bcx.ins().trap(TrapCode::unwrap_user(1));
    bcx.finalize(isa.frontend_config());
    let code = context
        .compile(
            isa,
            &mut cranelift_codegen::control::ControlPlane::default(),
        )
        .map_err(|error| format!("trap stub: {:?}", error.inner))?;
    let alignment = isa.function_alignment().minimum.into();
    Ok((alignment, code.code_buffer().to_vec()))
}

/// The function and the companions its job defines with it.
fn symbols(function: &UserFn) -> impl Iterator<Item = FuncId> {
    std::iter::once(function.id)
        .chain(function.drop_callback)
        .chain(function.resume)
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
            locals: &locals,
            stack: Vec::new(),
            terminated: false,
            loop_targets: Vec::new(),
            checked_targets: Vec::new(),
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
        self.note_stable_inputs(inputs);
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

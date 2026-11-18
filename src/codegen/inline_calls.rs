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

/// The same bound for a function called from several places: about the
/// size of the call it replaces, so each copy costs little.
const MAX_SHARED_OPERATIONS: usize = 16;

/// A larger bound for a function called from several places, met only while
/// its copies together stay within [`MAX_SHARED_GROWTH`] (issue #770). A
/// bounds-checked accessor such as `Bytes.get`, or a short arithmetic helper
/// in a hot loop, is just above [`MAX_SHARED_OPERATIONS`].
const MAX_SHARED_GROWTH_OPERATIONS: usize = 24;

/// Operations in all copies of a function inlined by
/// [`MAX_SHARED_GROWTH_OPERATIONS`]: ten call sites at that bound. A helper
/// called from dozens of places, as in a parser, stays a call.
const MAX_SHARED_GROWTH: usize = 256;

/// Whether `name` is a constructor: a class's `new` or its `__init__`.
fn constructor(name: &str) -> bool {
    crate::record::is_method(name) && (name.ends_with(".new") || name.ends_with(".__init__"))
}

fn marker(op: &Op) -> bool {
    matches!(
        op,
        Op::Assumed(_)
            | Op::Proved(..)
            | Op::Site(_)
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
            | Op::Reborrow(_)
            | Op::ForwardsReferences
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
    active: Vec<&'a str>,
    /// Callees reaching a recursive component cannot use retained-call fallback.
    recursive: crate::hash::HashSet<&'a str>,
    /// Proven before planning; retained calls cannot need a dynamic caller site.
    context_free: crate::hash::HashSet<&'a str>,
    /// Inline copies that fit only while their context-free callees stay calls.
    native_calls: crate::hash::HashSet<&'a str>,
    /// Leaf substitutions are already selected before this larger-body plan.
    leaves: crate::hash::HashSet<&'a str>,
    /// An inlined outer body may substitute a terminal call only when its
    /// own pre-call cleanup has no work, matching `lower_tail_call`.
    cleanup_free: crate::hash::HashSet<&'a str>,
}

/// Mark the functions to lower at their call sites in optimized builds:
/// bounded in size and not recursive (issues #548 and #722). The bound
/// depends on how often the body would be copied, so compile time grows
/// little: a constructor is small by construction, a function with one call
/// site is usually lowered only there, and one called from several places
/// must be about as small as the call it replaces, or a little larger while
/// its copies together stay small.
///
/// A cleanup-free wrapper may substitute its terminal callees, or keep
/// context-independent callees native when expanding them exceeds its bound.
/// Retained terminal calls return into the inline copy, never transfer out of
/// its containing frame. Generators and bodies defining functions stay calls.
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
        active: Vec::new(),
        recursive: Default::default(),
        context_free: bodies
            .iter()
            .filter(|(name, _)| functions[name].reads_no_caller)
            .map(|(name, _)| name.as_str())
            .collect(),
        native_calls: Default::default(),
        leaves: bodies
            .iter()
            .filter(|(name, _)| functions[name].inline.is_some())
            .map(|(name, _)| name.as_str())
            .collect(),
        cleanup_free: bodies
            .iter()
            .filter(|(name, _)| {
                let function = &functions[name];
                function
                    .sig
                    .inputs
                    .iter()
                    .map(|(_, ty)| ty)
                    .chain(function.locals.iter())
                    .all(|ty| !ty.managed())
            })
            .map(|(name, _)| name.as_str())
            .collect(),
    };
    // Declaration order keeps the outcome deterministic.
    for (name, _) in bodies {
        if plan.candidates.contains_key(name.as_str()) {
            plan.size(name);
        }
    }
    for (name, body) in &plan.candidates {
        if let Some(State::Done(Some(_))) = plan.states.get(name) {
            let function = functions.get_mut(*name).unwrap();
            function.inline_body = Some(Arc::clone(body));
            function.inline_native_calls = plan.native_calls.contains(name);
        }
    }
}

impl<'a> Plan<'a> {
    /// Whether `name`, with `size` operations counting its inlined callees,
    /// is small enough to copy to each of its call sites.
    fn fits(&self, name: &str, size: usize) -> bool {
        let sites = self.sites.get(name).copied().unwrap_or(0);
        if constructor(name) {
            size <= MAX_CONSTRUCTOR_OPERATIONS
        } else if sites == 1 {
            size <= MAX_SINGLE_SITE_OPERATIONS
        } else {
            size <= MAX_SHARED_OPERATIONS
                || (size <= MAX_SHARED_GROWTH_OPERATIONS && sites * size <= MAX_SHARED_GROWTH)
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
                // Every member must keep its native tail-transfer boundary.
                // Keeping only the revisited head native would let another
                // member inline a retained ordinary call back to that head,
                // turning mutual tail recursion into growing native frames.
                let start = self
                    .active
                    .iter()
                    .position(|&active| active == name)
                    .unwrap();
                for &member in &self.active[start..] {
                    self.states.insert(member, State::Done(None));
                    self.recursive.insert(member);
                }
                return None;
            }
            None => {}
        }
        let body = *self.candidates.get(name)?;
        self.states.insert(name, State::Visiting);
        self.active.push(name);
        let mut ops = Vec::new();
        crate::threading::walk(body, &mut |op| ops.push(op));
        let mut total = Some(0usize);
        // Prefer the established full-expansion plan. The alternative keeps
        // diagnostic-independent helpers behind their existing ABI;
        // only the wrapper grows its callers, under the same size bounds.
        let mut retained = Some(0usize);
        for op in ops {
            if marker(op) {
                continue;
            }
            let cost = match op {
                Op::TailCall(callee) if self.cleanup_free.contains(name) => {
                    if self.leaves.contains(callee.as_str()) {
                        Some(1)
                    } else {
                        self.size(callee)
                    }
                }
                Op::TailCall(_) | Op::TailCallIndirect(_) | Op::Yield(_) | Op::DefineFn(..) => None,
                Op::Call(callee) if self.candidates.contains_key(callee.as_str()) => {
                    Some(self.size(callee).unwrap_or(1))
                }
                _ => Some(1),
            };
            if let Op::Call(callee) | Op::TailCall(callee) = op
                && self.recursive.contains(callee.as_str())
            {
                self.recursive.insert(name);
            }
            let retained_cost = match op {
                Op::TailCall(callee) | Op::Call(callee)
                    if self.cleanup_free.contains(name)
                        && self.context_free.contains(callee.as_str()) =>
                {
                    Some(1)
                }
                _ => cost,
            };
            total = total.zip(cost).map(|(total, cost)| total + cost);
            retained = retained
                .zip(retained_cost)
                .map(|(total, cost)| total + cost);
        }
        let active = self.active.pop();
        debug_assert_eq!(active, Some(name));
        // A recursive call found below marked this function as staying a call.
        let size = match self.states.get(name) {
            Some(State::Done(None)) => None,
            _ => total.filter(|&total| self.fits(name, total)).or_else(|| {
                retained
                    .filter(|&size| !self.recursive.contains(name) && self.fits(name, size))
                    .inspect(|_| {
                        self.native_calls.insert(name);
                    })
            }),
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
        captures: &[cranelift_codegen::ir::Value],
    ) -> Result<()> {
        let functions = self.user_fns;
        let decl = &functions[name];
        let inputs = decl.sig.inputs.len();
        let caller = self.inline_caller(track_caller && decl.names_caller_in_sites);
        let saved = if track_caller && caller.is_none() {
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
        let width = self.module.simd_width();
        let portable_simd =
            portable_simd::State::new(body, &locals, inputs, self.bcx, optimizes, width);
        let packs = self.module.packs_statements();
        let pairs = packed::Pairs::new(body, &locals, inputs, self.bcx, packs);
        let mut addressed = addressed_locals(body, &locals);
        for (index, addressed) in addressed.iter_mut().enumerate() {
            if portable_simd.promoted(index as u8) {
                *addressed = false;
            }
        }
        let local_frame = local_frame(self.bcx, &locals, &addressed, inputs);
        let return_storage = self.call_result_storage(&decl.sig.outputs);
        let mut scalar_captures =
            scalar_captures::Captures::new(body, &locals, inputs, functions, self.bcx);
        // These are immutable by-value scalar snapshots. Retain the actual
        // reference arguments, but substitute direct reads in this callee.
        scalar_captures.bind_reads(&args, captures);
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
            assumed_index: false,
            loop_targets: Vec::new(),
            checked_targets: Vec::new(),
            generator: None,
            local_frame,
            return_storage,
            return_types: &decl.sig.outputs,
            collection_scratch: self.collection_scratch,
            fresh_load: None,
            tags: Default::default(),
            scalar_captures,
            portable_simd,
            pairs,
            slot_copies: Vec::new(),
            empty_slot: Some((empty_word, empty_double)),
            exits: Some(exits::Exits::returning_to(
                returned,
                body,
                decl.inline_native_calls,
            )),
            // A fact about an SSA value holds wherever the value is used, so
            // the callee keeps what the caller knows about its arguments.
            counters: induction::Facts {
                nonnegative: std::mem::take(&mut self.counters.nonnegative),
                at_most: std::mem::take(&mut self.counters.at_most),
                byte_literals: std::mem::take(&mut self.counters.byte_literals),
                reference_words: std::mem::take(&mut self.counters.reference_words),
                ..Default::default()
            },
            position: match caller {
                Some(caller) => sites::Position::inlined(decl.source.clone(), caller),
                None => sites::Position::in_file(decl.source.clone(), decl.standard_library),
            },
            trap_blocks: HashMap::default(),
            moves: Default::default(),
            joins: std::mem::take(&mut self.joins),
        };
        let lowered = callee.lower_body(body, inputs);
        self.joins = std::mem::take(&mut callee.joins);
        self.collection_scratch = callee.collection_scratch;
        self.counters.nonnegative = std::mem::take(&mut callee.counters.nonnegative);
        self.counters.at_most = std::mem::take(&mut callee.counters.at_most);
        self.counters.byte_literals = std::mem::take(&mut callee.counters.byte_literals);
        self.counters.reference_words = std::mem::take(&mut callee.counters.reference_words);
        let callee_loops = lowered
            .map_err(|e| -> Box<dyn Error> { format!("in inlined `{name}`: {e}").into() })?;
        if callee_loops && let Some(exits) = &mut self.exits {
            exits.mark_loop();
        }
        self.bcx.switch_to_block(returned);
        // A `?` right after the call may move returns out of this block.
        self.seal_join_later(returned, matches!(&decl.sig.outputs[..], [Ty::Enum(_)]));
        self.bcx.set_srcloc(srcloc);
        self.leave_call(saved);
        let results = self.bcx.block_params(returned).to_vec();
        for (value, ty) in results.into_iter().zip(&decl.sig.outputs) {
            self.stack.push((value, ty.clone()));
        }
        Ok(())
    }

    /// Lower an inlined body as [`build_user_function`] lowers a function's.
    /// Returns whether it has a loop, counting loops of callees inlined
    /// into it at any depth, so the caller's later exits stage their values.
    fn lower_body(&mut self, body: &[Op], inputs: usize) -> Result<bool> {
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
        // Read before `finish_exits` takes the exits.
        let has_loop = self.exits.as_ref().is_some_and(|e| e.has_loop());
        self.finish_exits()?;
        self.seal_trap_blocks();
        self.finish_portable_simd();
        Ok(has_loop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_calls_require_context_independence_cleanup_freedom_and_bounded_growth() {
        let large: Arc<[Op]> = vec![Op::Dup; MAX_SINGLE_SITE_OPERATIONS + 1].into();
        let mut wrapper = vec![Op::Dup; 20];
        wrapper.push(Op::TailCall("large".into()));
        let wrapper: Arc<[Op]> = wrapper.into();
        for (safe, cleanup_free, sites, expected) in [
            (true, true, 3, Some(21)),
            (false, true, 3, None),
            (true, false, 3, None),
            (true, true, 20, None),
        ] {
            let mut plan = Plan {
                candidates: [("wrapper", &wrapper), ("large", &large)]
                    .into_iter()
                    .collect(),
                sites: [("wrapper", sites), ("large", 2)].into_iter().collect(),
                states: Default::default(),
                active: Vec::new(),
                recursive: Default::default(),
                leaves: Default::default(),
                context_free: if safe {
                    ["large"].into_iter().collect()
                } else {
                    Default::default()
                },
                cleanup_free: if cleanup_free {
                    ["wrapper"].into_iter().collect()
                } else {
                    Default::default()
                },
                native_calls: Default::default(),
            };
            assert_eq!(
                plan.size("wrapper"),
                expected,
                "{safe}, {cleanup_free}, {sites}"
            );
            assert_eq!(plan.native_calls.contains("wrapper"), expected.is_some());
        }
    }

    #[test]
    fn context_independent_recursion_still_keeps_its_frame() {
        let body: Arc<[Op]> = vec![Op::TailCall("recursive".into())].into();
        let mut plan = Plan {
            candidates: [("recursive", &body)].into_iter().collect(),
            sites: [("recursive", 1)].into_iter().collect(),
            states: Default::default(),
            active: Vec::new(),
            recursive: Default::default(),
            leaves: Default::default(),
            context_free: ["recursive"].into_iter().collect(),
            cleanup_free: ["recursive"].into_iter().collect(),
            native_calls: Default::default(),
        };
        assert_eq!(plan.size("recursive"), None);
        assert!(plan.native_calls.is_empty());
    }

    #[test]
    fn context_independent_mutual_recursion_keeps_every_transfer_native() {
        let first: Arc<[Op]> = vec![Op::If {
            then: vec![Op::TailCall("second".into())].into(),
            elifs: Arc::from([]),
            otherwise: vec![Op::TailCall("third".into())].into(),
        }]
        .into();
        let second: Arc<[Op]> = vec![Op::TailCall("first".into())].into();
        let third: Arc<[Op]> = vec![Op::TailCall("second".into())].into();
        let mut plan = Plan {
            candidates: [("first", &first), ("second", &second), ("third", &third)]
                .into_iter()
                .collect(),
            sites: [("first", 1), ("second", 2), ("third", 1)]
                .into_iter()
                .collect(),
            states: Default::default(),
            active: Vec::new(),
            recursive: Default::default(),
            leaves: Default::default(),
            context_free: ["first", "second", "third"].into_iter().collect(),
            cleanup_free: ["first", "second", "third"].into_iter().collect(),
            native_calls: Default::default(),
        };
        assert_eq!(plan.size("first"), None);
        assert_eq!(plan.size("second"), None);
        assert_eq!(plan.size("third"), None);
        assert!(plan.native_calls.is_empty());
    }
}

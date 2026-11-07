//! Native stack budgets for calls into checked serialization code.
//!
//! Checks run in the caller, before the callee's prologue reserves its frame.
//! The budget includes the longest unguarded call segment, the failure helper,
//! and recovery space. A missing frame or an unguarded cycle fails closed.
use super::*;
use crate::collection::CollectionOp;
use crate::hash::HashSet;
use std::sync::{Arc, Mutex};

pub(super) type Requests = Arc<Mutex<HashMap<(FuncId, FuncId), DataId>>>;

const RECOVERY_BYTES: u64 = 65536;

#[derive(Default)]
pub(super) struct Budgets {
    pub requests: Requests,
    protected: HashSet<FuncId>,
    calls: HashMap<FuncId, Vec<Edge>>,
    unknown: HashSet<FuncId>,
    inlined: HashSet<FuncId>,
    frames: HashMap<FuncId, Option<u64>>,
    tail_arguments: HashMap<FuncId, u64>,
}

impl Budgets {
    pub(super) fn protected_names(ops: &[Op]) -> HashSet<String> {
        let mut names = HashSet::default();
        crate::threading::walk(ops, &mut |op| {
            if let Op::Collection(CollectionOp::SerialStackReady { callee, .. }) = op {
                names.insert(callee.clone());
            }
        });
        names
    }

    pub(super) fn prepare(
        &mut self,
        bodies: &[(String, Arc<[Op]>)],
        functions: &HashMap<String, UserFn>,
        protected: &HashSet<String>,
        box_releasers: &[(Ty, FuncId)],
    ) {
        if protected.is_empty() {
            return;
        }
        self.protected = protected.iter().map(|name| functions[name].id).collect();
        for (name, body) in bodies {
            let function = &functions[name];
            let mut graph = Graph::new(functions, box_releasers);
            graph.body(body);
            // Ownership cleanup can call native destructor adapters without an
            // explicit Call op. Include even conservative possible releases.
            for ty in function.locals.iter().chain(&function.sig.outputs) {
                graph.release(ty);
            }
            for (_, ty) in &function.sig.inputs {
                graph.release(ty);
            }
            if graph.unknown {
                self.unknown.insert(function.id);
            }
            let arguments = graph
                .arguments
                .max(crate::tail_abi::layout(&function.sig).inputs.len() as u64);
            self.calls.insert(function.id, graph.edges);
            self.tail_arguments.insert(function.id, arguments * 16);
            if function.inline.is_some() || function.inline_body.is_some() {
                self.inlined.insert(function.id);
            }
            if let Some(adapter) = function.drop_callback {
                self.calls.insert(adapter, vec![Edge::direct(function.id)]);
                self.tail_arguments.insert(adapter, 16);
            }
        }
        // These adapters release heap children through the runtime's iterative
        // destruction queue and never call user code (box_release::children).
        for (_, adapter) in box_releasers {
            self.calls.insert(*adapter, Vec::new());
            self.tail_arguments.insert(*adapter, 16);
        }
    }

    pub(super) fn frame(&mut self, function: FuncId, bytes: Option<u64>) {
        if !self.protected.is_empty() {
            self.frames.insert(function, bytes);
        }
    }

    fn segment(
        &self,
        function: FuncId,
        active: &mut HashSet<FuncId>,
        memo: &mut HashMap<FuncId, Option<u64>>,
        failure: bool,
    ) -> Option<u64> {
        if self.unknown.contains(&function) {
            return None;
        }
        if let Some(value) = memo.get(&function) {
            return *value;
        }
        if !active.insert(function) {
            return None;
        }
        let result = (|| {
            let frame = match self.frames.get(&function) {
                Some(frame) => (*frame)?,
                // Inlined functions' storage is already in their callers'
                // measured frames, but their non-inlined callees still count.
                None if self.inlined.contains(&function) => 0,
                None => return None,
            };
            let mut children = 0;
            for edge in self.calls.get(&function)? {
                // A reject bridge cannot re-enter a checked codec: a failing
                // nested check can re-enter rejection again without progress.
                if failure && (edge.guarded || self.protected.contains(&edge.callee)) {
                    return None;
                }
                if !edge.guarded {
                    children = children.max(self.segment(edge.callee, active, memo, failure)?);
                }
            }
            frame
                .checked_add(*self.tail_arguments.get(&function)?)?
                .checked_add(children)
        })();
        active.remove(&function);
        memo.insert(function, result);
        result
    }

    pub(super) fn define(&self, module: &mut ObjectModule) -> Result<()> {
        let mut requests: Vec<_> = self
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(key, id)| (*key, *id))
            .collect();
        requests.sort_by_key(|(_, id)| id.as_u32());
        let mut memo = HashMap::default();
        let mut failure_memo = HashMap::default();
        for ((callee, failure), id) in requests {
            let failure_bytes = self.segment(failure, &mut HashSet::default(), &mut failure_memo, true)
                .ok_or("cannot safely report serialization stack exhaustion: the adapter's reject path re-enters a checked codec, has recursive or unknown callbacks, or has an unavailable frame budget")?;
            let required = self
                .segment(callee, &mut HashSet::default(), &mut memo, false)
                .and_then(|target| target.checked_add(failure_bytes))
                .and_then(|bytes| bytes.checked_add(RECOVERY_BYTES))
                .unwrap_or(u64::MAX);
            let mut data = DataDescription::new();
            data.set_align(8);
            data.define(required.to_le_bytes().to_vec().into_boxed_slice());
            module.define_data(id, &data)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Edge {
    callee: FuncId,
    guarded: bool,
}

impl Edge {
    fn direct(callee: FuncId) -> Self {
        Self {
            callee,
            guarded: false,
        }
    }
}

struct Graph<'a> {
    functions: &'a HashMap<String, UserFn>,
    box_releasers: &'a [(Ty, FuncId)],
    edges: Vec<Edge>,
    arguments: u64,
    unknown: bool,
    released: HashSet<(u8, Arc<str>)>,
}

impl<'a> Graph<'a> {
    fn new(functions: &'a HashMap<String, UserFn>, box_releasers: &'a [(Ty, FuncId)]) -> Self {
        Self {
            functions,
            box_releasers,
            edges: Vec::new(),
            arguments: 0,
            unknown: false,
            released: HashSet::default(),
        }
    }

    fn call(&mut self, name: &str, guarded: bool) {
        let Some(target) = self.functions.get(name) else {
            self.unknown = true;
            return;
        };
        self.edges.push(Edge {
            callee: target.id,
            guarded,
        });
        self.arguments = self
            .arguments
            .max(crate::tail_abi::layout(&target.sig).inputs.len() as u64);
        // A returned temporary can be released before it ever reaches a local.
        for output in &target.sig.outputs {
            self.release(output);
        }
    }

    /// Recognize the frontend's exact guard/check pair, not a global target
    /// name. Another call to the same function can still be unguarded.
    fn checked_call<'b>(check: &'b Op, branch: &'b Op) -> Option<(&'b str, &'b [Op])> {
        let Op::Collection(CollectionOp::SerialStackReady { callee, failure }) = check else {
            return None;
        };
        let Op::If {
            then,
            elifs,
            otherwise,
        } = branch
        else {
            return None;
        };
        if !elifs.is_empty()
            || !otherwise
                .iter()
                .any(|op| matches!(op, Op::Call(name) if name == failure))
        {
            return None;
        }
        let (last, prefix) = then.split_last()?;
        if !matches!(last, Op::Call(name) | Op::TailCall(name) if name == callee)
            || !prefix
                .iter()
                .all(|op| matches!(op, Op::ForwardsReferences | Op::Passes(_) | Op::Forwards(_)))
        {
            return None;
        }
        Some((callee, otherwise))
    }

    fn body(&mut self, ops: &[Op]) {
        let mut index = 0;
        while index < ops.len() {
            let op = &ops[index];
            if let Some(branch) = ops.get(index + 1)
                && let Some((callee, otherwise)) = Self::checked_call(op, branch)
            {
                self.call(callee, true);
                self.body(otherwise);
                index += 2;
                continue;
            }
            match op {
                Op::Call(name) | Op::TailCall(name) => self.call(name, false),
                Op::ClosureCall(closure) => self.call(&closure.name, false),
                // Runtime callback adapters and erased targets need their own
                // measured graph before they can be accepted in a checked
                // segment. Conservatively reject rather than omit an edge.
                Op::CallIndirect(_)
                | Op::TailCallIndirect(_)
                | Op::BoxedCallableCall(_)
                | Op::ProtocolBoxCall(..)
                | Op::Next(_)
                | Op::ForeignCall { .. }
                | Op::Thread(_)
                | Op::Executor(_) => self.unknown = true,
                Op::Assumed(inner) => self.body(std::slice::from_ref(inner)),
                // Each nested definition is analyzed independently in prepare.
                Op::DefineFn(..) => {}
                Op::Match(arms) => {
                    for arm in arms.iter() {
                        self.body(&arm.body);
                    }
                }
                Op::Loop { condition, body } => {
                    self.body(condition);
                    self.body(body);
                }
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    self.body(then);
                    for arm in elifs.iter() {
                        self.body(&arm.condition);
                        self.body(&arm.body);
                    }
                    self.body(otherwise);
                }
                Op::Try {
                    source,
                    target,
                    cleanup,
                } => {
                    self.release(&Ty::Enum((**source).clone()));
                    self.release(&Ty::Enum((**target).clone()));
                    self.body(cleanup);
                }
                Op::CheckedArith { cleanup, .. } => self.body(cleanup),
                Op::CheckedBlock { body, .. } | Op::CheckedFunction { body, .. } => self.body(body),
                Op::Collection(collection) => {
                    if matches!(
                        collection,
                        CollectionOp::ListSortBy(_) | CollectionOp::Formatted(_)
                    ) {
                        self.unknown = true;
                    }
                    let (inputs, output) = collection.signature();
                    for ty in inputs {
                        self.release(&ty);
                    }
                    self.release(&output);
                }
                Op::Class(operation) => {
                    if let Some((inputs, output)) = operation.signature() {
                        for ty in inputs {
                            self.release(&ty);
                        }
                        self.release(&output);
                    }
                }
                Op::Enum(operation) => {
                    if let Some((inputs, output)) = operation.signature() {
                        for ty in inputs {
                            self.release(&ty);
                        }
                        self.release(&output);
                    }
                }
                Op::Box(operation) => {
                    let (inputs, output) = operation.signature();
                    for ty in inputs {
                        self.release(&ty);
                    }
                    self.release(&output);
                }
                Op::ClosureNew(closure) => self.release(&Ty::Closure(closure.clone())),
                _ => {}
            }
            index += 1;
        }
    }

    /// Include possible user-code releases and their compiler-generated entry
    /// frames. The runtime drains heap children iteratively, so distinct
    /// children share the maximum callback budget rather than nesting it.
    fn release(&mut self, ty: &Ty) {
        match ty {
            Ty::Ref(..) | Ty::View(..) => {}
            Ty::Class(class) => {
                if !self.released.insert((0, class.name.clone())) {
                    return;
                }
                let Some(definition) = class.try_get() else {
                    self.unknown = true;
                    return;
                };
                if let Some(name) = &definition.destructor {
                    match self.functions.get(name).and_then(|f| f.drop_callback) {
                        Some(callee) => self.edges.push(Edge::direct(callee)),
                        None => self.unknown = true,
                    }
                }
                for (_, field) in &definition.fields {
                    self.release(field);
                }
            }
            Ty::Enum(sum) => {
                if !self.released.insert((1, sum.name.clone())) {
                    return;
                }
                let Some(definition) = sum.try_get() else {
                    self.unknown = true;
                    return;
                };
                for variant in &definition.variants {
                    for field in &variant.fields {
                        self.release(field);
                    }
                }
            }
            Ty::Box(content) => {
                if let Some((_, callee)) =
                    self.box_releasers.iter().find(|(ty, _)| ty == &**content)
                {
                    self.edges.push(Edge::direct(*callee));
                }
                self.release(content);
            }
            Ty::List(content) | Ty::Set(content) | Ty::Shared(content) | Ty::Clist(content) => {
                self.release(content)
            }
            Ty::Dict(key, value) => {
                self.release(key);
                self.release(value);
            }
            Ty::Closure(closure) => {
                for (_, field) in &closure.captures {
                    self.release(field);
                }
            }
            _ if ty.release_runs_code() => self.unknown = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Budgets, FuncId, FuncId) {
        let a = FuncId::from_u32(0);
        let b = FuncId::from_u32(1);
        let mut budgets = Budgets::default();
        budgets.protected.insert(a);
        budgets.frames.extend([(a, Some(100)), (b, Some(200))]);
        budgets.tail_arguments.extend([(a, 16), (b, 32)]);
        budgets.calls.extend([
            (a, vec![Edge::direct(b)]),
            (
                b,
                vec![Edge {
                    callee: a,
                    guarded: true,
                }],
            ),
        ]);
        (budgets, a, b)
    }

    #[test]
    fn only_verified_callsite_edges_stop_recursion() {
        let (mut budgets, a, b) = sample();
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default(), false),
            Some(348)
        );
        // Same protected target, but a call without its own check. A global
        // protected-name cut would incorrectly accept this recursive graph.
        budgets.calls.get_mut(&b).unwrap().push(Edge::direct(a));
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default(), false),
            None
        );
    }

    #[test]
    fn rejection_cannot_reenter_checked_code_even_through_a_guard() {
        let (mut budgets, _, b) = sample();
        assert_eq!(
            budgets.segment(b, &mut HashSet::default(), &mut HashMap::default(), true),
            None
        );
        budgets.calls.get_mut(&b).unwrap()[0].guarded = false;
        assert_eq!(
            budgets.segment(b, &mut HashSet::default(), &mut HashMap::default(), true),
            None
        );
    }

    #[test]
    fn missing_frames_unknown_callbacks_and_inlining_are_conservative() {
        let (mut budgets, a, b) = sample();
        budgets.frames.remove(&b);
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default(), false),
            None
        );
        budgets.inlined.insert(b);
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default(), false),
            Some(148)
        );
        budgets.unknown.insert(b);
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default(), false),
            None
        );
        assert_eq!(
            budgets.segment(a, &mut HashSet::default(), &mut HashMap::default(), true),
            None
        );
    }

    #[test]
    fn guard_recognition_requires_the_matching_adjacent_check_and_exact_call() {
        let check = Op::Collection(CollectionOp::SerialStackReady {
            callee: "codec".into(),
            failure: "reject".into(),
        });
        let branch = |then: Vec<Op>, otherwise: Vec<Op>| Op::If {
            then: then.into(),
            elifs: Arc::from([]),
            otherwise: otherwise.into(),
        };
        let good = branch(
            vec![Op::ForwardsReferences, Op::Call("codec".into())],
            vec![Op::Call("reject".into())],
        );
        assert!(Graph::checked_call(&check, &good).is_some());
        let wrong_target = branch(
            vec![Op::Call("other".into())],
            vec![Op::Call("reject".into())],
        );
        assert!(Graph::checked_call(&check, &wrong_target).is_none());
        let unchecked_work = branch(
            vec![Op::Call("other".into()), Op::Call("codec".into())],
            vec![Op::Call("reject".into())],
        );
        assert!(Graph::checked_call(&check, &unchecked_work).is_none());
        let wrong_rejection = branch(
            vec![Op::Call("codec".into())],
            vec![Op::Call("other".into())],
        );
        assert!(Graph::checked_call(&check, &wrong_rejection).is_none());
    }

    #[test]
    fn widened_call_budget_includes_the_hidden_inline_result_argument() {
        let signature = Arc::new(FnSig {
            inputs: (0..7).map(|i| (format!("arg{i}"), Ty::I64)).collect(),
            outputs: vec![crate::sum::tuple(vec![Ty::I64, Ty::I64, Ty::I64])],
        });
        let native_arguments = user_fn_signature(&signature).params.len() as u64;
        assert_eq!(native_arguments, 8);
        let functions = HashMap::from_iter([(
            "callee".into(),
            UserFn {
                inline: None,
                inline_body: None,
                column_predicate: None,
                standard_library: false,
                drop_callback: None,
                generator: None,
                resume: None,
                frame_mask: None,
                id: FuncId::from_u32(0),
                sig: signature,
                locals: Arc::from([]),
                source: None,
                preserves_caller: false,
                reads_no_caller: false,
            },
        )]);
        let mut graph = Graph::new(&functions, &[]);
        graph.call("callee", false);
        assert_eq!(graph.arguments, native_arguments);
    }

    #[test]
    fn runtime_callback_operations_fail_closed() {
        let functions = HashMap::default();
        let mut graph = Graph::new(&functions, &[]);
        graph.body(&[Op::Next(0)]);
        assert!(graph.unknown);
    }
}

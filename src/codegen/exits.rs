//! Shared exits of a user function (issues #407, #417 and #419).
//!
//! A `?` failure runs its cleanup ops, then, like every `return` and tail
//! call, releases each owned local that may still hold a value and leaves.
//! A `return`, `break` or `continue` inside scopes first runs the scope
//! cleanup the front end marks with [`Op::Leaving`], and a `continue` then
//! its loop's step. Lowered at each exit, that code is proportional to the
//! live locals, so a function with *n* exits and *n* owned locals grows
//! quadratically. Instead each exit stages the values it carries and jumps
//! into a chain of shared blocks.
//!
//! An exit's work is a list of steps: its cleanup ops, split after each
//! `DropLocal`, then, unless it stays in the function, one release per local
//! that may hold a value, in reverse slot order. One block performs each
//! distinct (step, rest of the list) pair and jumps to the block for the
//! rest; the last jumps to the exit's target: a return, a tail call, or a
//! loop's exit block or header. Successive exits in a scope release the same
//! values in the same order after their newest locals, so their lists share
//! suffixes and the chain is a ladder entered at each exit's newest live
//! value. Lists with no common suffix still share identical ones.
//!
//! Direct tail calls whose callees take the same carried values share one
//! target, so their releases are shared too. Each exit names its callee's
//! index in a variable, and the target dispatches on it when it has more than
//! one callee.
//!
//! Behavior matches lowering each exit inline:
//! - Steps run in the same order, from the same operand stack (empty). The
//!   carried values are staged first; nothing a step does can reach them.
//! - An exit omits the release of a local whose value it knows is empty, as
//!   [`Lowerer::release`] would, and of one its cleanup drops; likewise a
//!   cleanup's drop of such a local.
//! - A shared block cannot name one source position, so exits pass their
//!   trap and caller records in two variables, which the blocks read until a
//!   marker of their own fixes the position ([`sites::Position::share`]).
//!   After a context's exit, scope cleanup restores the exiting statement's
//!   position before the next drop. That marker differs between exits, so
//!   exits pass it in two more variables and a [`Step::Restore`] reads them.
//!   Line tables likewise give a shared block no line until its own marker.
//! - Shared blocks are lowered after every predecessor exists, and sealed
//!   first, so a local every predecessor leaves empty still reads as the empty
//!   constant (issue #391): a function's blocks after its body, a loop's
//!   before its exit block is sealed.
use super::*;
use cranelift_codegen::ir::{
    FuncRef, JumpTableData, SigRef, Signature, Type, Value as NativeValue, types,
};
use std::ops::Range;

#[derive(Default)]
pub(super) struct Exits {
    /// Trap and caller records of the exit being taken.
    sites: Option<(Variable, Variable)>,
    /// Records of the position a [`Step::Restore`] returns to.
    restore: Option<(Variable, Variable)>,
    /// Which callee of an [`End::Calls`] target the exit being taken calls.
    callee: Option<Variable>,
    /// The scope cleanup gathered since an `Op::Leaving`, until its `Return`,
    /// `Break` or `Continue`.
    pending: Option<Vec<Op>>,
    /// Exits leaving the function.
    function: Ladder,
    /// Where the function's returns go when it is lowered into a caller
    /// ([`super::inline_calls`]): a block taking the returned values.
    pub(super) returns_to: Option<Block>,
    /// `break`s and `continue`s of each enclosing loop, innermost last.
    loops: Vec<Ladder>,
}

impl Exits {
    /// Exits of a body lowered into its caller, returning to `block`.
    pub(super) fn returning_to(block: Block) -> Self {
        Self {
            returns_to: Some(block),
            ..Default::default()
        }
    }
}

#[derive(Default)]
struct Ladder {
    targets: Vec<Target>,
    /// Each step follows the step or target it jumps to.
    steps: Vec<Shared>,
    /// The steps jumping to each target.
    entering: HashMap<Next, Vec<usize>>,
}

struct Target {
    block: Block,
    /// The carried values: returned values, or a tail call's operands.
    types: Vec<Type>,
    end: End,
}

enum End {
    Return,
    /// A block the loop lowers: its exit block, or the header a `continue`
    /// returns to.
    Jump(Block),
    /// Tail calls of callees taking the same carried values. Exits calling
    /// different callees share their releases; the target dispatches on
    /// [`Exits::callee`] when there is more than one.
    Calls(Vec<Callee>),
    /// The callee is the first carried value.
    CallIndirect(Signature, SigRef, bool),
}

/// A tail call's callee, and whether the call names this function's caller.
/// Each declaration of a callee makes a new reference, so the function's id
/// identifies it.
#[derive(Clone, Copy)]
struct Callee {
    id: FuncId,
    func: FuncRef,
    track_caller: bool,
}

impl End {
    /// Whether an exit ending in `self` can use a target ending in `other`.
    fn joins(&self, other: &End) -> bool {
        match (self, other) {
            (End::Return, End::Return) | (End::Calls(_), End::Calls(_)) => true,
            (End::Jump(a), End::Jump(b)) => a == b,
            (End::CallIndirect(a, _, x), End::CallIndirect(b, _, y)) => a == b && x == y,
            _ => false,
        }
    }
}

struct Shared {
    block: Block,
    /// How many of the block's leading parameters are carried values; SSA
    /// construction appends more for the variables the block reads.
    carried: usize,
    step: Step,
    next: Next,
}

enum Step {
    /// Cleanup ops of a `?`, `return`, `break` or `continue`, and a
    /// `continue`'s loop step.
    Ops(Arc<[Op]>, Range<usize>),
    /// Return to the exiting statement's position, as its cleanup's marker
    /// after a context exit does.
    Restore,
    /// Release a local without emptying it: the function is leaving.
    Release(u8),
}

impl Step {
    fn same(&self, other: &Step) -> bool {
        match (self, other) {
            (Step::Ops(a, x), Step::Ops(b, y)) => a[x.clone()]
                .iter()
                .filter(|op| !inert(op))
                .eq(b[y.clone()].iter().filter(|op| !inert(op))),
            (Step::Restore, Step::Restore) => true,
            (Step::Release(a), Step::Release(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Next {
    Step(usize),
    Target(usize),
}

impl Ladder {
    fn block(&self, next: Next) -> Block {
        match next {
            Next::Step(i) => self.steps[i].block,
            Next::Target(i) => self.targets[i].block,
        }
    }
}

#[derive(Clone, Copy)]
enum Which {
    Function,
    Loop,
}

/// An ownership record, which lowers to nothing. The loans such records name
/// differ between exits.
fn inert(op: &Op) -> bool {
    matches!(
        op,
        Op::Loan(_) | Op::UseLoan(_) | Op::EndLoan(_) | Op::Access(..)
    )
}

/// The restoring marker of `cleanup`: the front end puts the exiting
/// statement's position right before the drop that follows a context's exit.
/// Turning any marker into a [`Step::Restore`] of its own position keeps its
/// effect, so this need only find the one that differs between exits.
fn restoring(cleanup: &[Op]) -> Option<crate::ownership::Site> {
    cleanup.windows(2).rev().find_map(|pair| match pair {
        [Op::Site(site), Op::DropLocal(_)] => Some(*site),
        _ => None,
    })
}

impl Lowerer<'_, '_> {
    /// Run `cleanup`, release the locals and return `values`. In a user
    /// function this jumps into the shared exit blocks; elsewhere it lowers
    /// everything inline.
    pub(super) fn leave(&mut self, values: Vec<StackEntry>, cleanup: &Arc<[Op]>) -> Result<()> {
        if self.exits.is_none() {
            for op in cleanup.iter() {
                self.lower(op)?;
            }
            self.return_values(values);
            return Ok(());
        }
        let returned = self.stage_results(values);
        let (mut steps, restore) = self.cleanup_steps(cleanup);
        self.push_releases(&mut steps, cleanup);
        // With nothing to share, a jump would only replace the return.
        if steps.is_empty() {
            let returns_to = self.exits.as_ref().and_then(|exits| exits.returns_to);
            self.return_to(returns_to, &returned);
        } else {
            self.take_exit(Which::Function, steps, restore, returned, End::Return)?;
        }
        // Inline lowering would leave the position at cleanup's last marker.
        self.follow_markers(cleanup);
        Ok(())
    }

    /// Whether `Op::Leaving` started gathering a scope cleanup.
    pub(super) fn gathering(&self) -> bool {
        self.exits
            .as_ref()
            .is_some_and(|exits| exits.pending.is_some())
    }

    /// Lower `Op::Leaving`: gather the cleanup up to its exit when the exit
    /// can share it.
    pub(super) fn start_leaving(&mut self) {
        if self.generator.is_some() {
            return;
        }
        if let Some(exits) = &mut self.exits {
            exits.pending = Some(Vec::new());
        }
    }

    /// While gathering a scope cleanup, take `op`: gather it, or lower the
    /// exit it ends. False when not gathering.
    pub(super) fn gather(&mut self, op: &Op) -> Result<bool> {
        let Some(pending) = self.exits.as_mut().and_then(|exits| exits.pending.as_mut()) else {
            return Ok(false);
        };
        if !matches!(op, Op::Return | Op::Break | Op::Continue) {
            pending.push(op.clone());
            return Ok(true);
        }
        let cleanup: Arc<[Op]> = Arc::from(std::mem::take(pending));
        if let Some(exits) = &mut self.exits {
            exits.pending = None;
        }
        if matches!(op, Op::Return) {
            self.leave(self.stack.clone(), &cleanup)?;
        } else {
            self.leave_loop(&cleanup, matches!(op, Op::Continue))?;
        }
        self.terminated = true;
        self.fresh_load = None;
        self.slot_copies.clear();
        Ok(true)
    }

    /// Leave the loop for its exit block, or for its header to continue.
    fn leave_loop(&mut self, cleanup: &Arc<[Op]>, continuing: bool) -> Result<()> {
        let (steps, restore) = self.cleanup_steps(cleanup);
        let &(header, exit) = self
            .loop_targets
            .last()
            .ok_or("loop control outside a loop")?;
        let target = if continuing { header } else { exit };
        if steps.is_empty() {
            self.bcx.ins().jump(target, &[]);
        } else {
            self.take_exit(Which::Loop, steps, restore, Vec::new(), End::Jump(target))?;
        }
        self.follow_markers(cleanup);
        Ok(())
    }

    /// Release the locals and tail-call `func` with `args` through the shared
    /// exit blocks. False when this lowers the call inline.
    pub(super) fn shared_tail_call(
        &mut self,
        id: FuncId,
        func: FuncRef,
        track_caller: bool,
        args: &[NativeValue],
    ) -> Result<bool> {
        let callee = Callee {
            id,
            func,
            track_caller,
        };
        self.shared_tail(args.to_vec(), End::Calls(vec![callee]))
    }

    /// As [`Self::shared_tail_call`] for an indirect call of `callee`.
    pub(super) fn shared_tail_call_indirect(
        &mut self,
        reference: SigRef,
        callee: NativeValue,
        args: &[NativeValue],
        track_caller: bool,
    ) -> Result<bool> {
        let signature = self.bcx.func.dfg.signatures[reference].clone();
        let carried = std::iter::once(callee)
            .chain(args.iter().copied())
            .collect();
        self.shared_tail(
            carried,
            End::CallIndirect(signature, reference, track_caller),
        )
    }

    fn shared_tail(&mut self, carried: Vec<NativeValue>, end: End) -> Result<bool> {
        if self.exits.is_none() {
            return Ok(false);
        }
        let mut steps = Vec::new();
        self.push_releases(&mut steps, &[]);
        if steps.is_empty() {
            return Ok(false);
        }
        self.take_exit(Which::Function, steps, None, carried, end)?;
        Ok(true)
    }

    /// `cleanup` split after each `DropLocal` and around its restoring
    /// markers, and the position those restore. The front end emits each
    /// context's receiver and exit together before the local's drop, so the
    /// operand stack is empty at every boundary.
    fn cleanup_steps(
        &mut self,
        cleanup: &Arc<[Op]>,
    ) -> (Vec<Step>, Option<crate::ownership::Site>) {
        let restore = restoring(cleanup);
        let mut steps = Vec::new();
        let mut start = 0;
        for (i, op) in cleanup.iter().enumerate() {
            match op {
                Op::Site(site)
                    if Some(*site) == restore
                        && matches!(cleanup.get(i + 1), Some(Op::DropLocal(_))) =>
                {
                    if cleanup[start..i].iter().any(|op| !inert(op)) {
                        steps.push(Step::Ops(cleanup.clone(), start..i));
                    }
                    steps.push(Step::Restore);
                    start = i + 1;
                }
                Op::DropLocal(slot) => {
                    // Dropping a local known to be empty emits nothing.
                    if cleanup[start..i].iter().any(|op| !inert(op)) || !self.known_empty(*slot) {
                        steps.push(Step::Ops(cleanup.clone(), start..i + 1));
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        if cleanup[start..].iter().any(|op| !inert(op)) {
            steps.push(Step::Ops(cleanup.clone(), start..cleanup.len()));
        }
        (steps, restore)
    }

    fn known_empty(&mut self, slot: u8) -> bool {
        let (var, ty) = &self.locals[slot as usize];
        if !ty.managed() {
            return true;
        }
        // Reading a variable here emits nothing; a frame load would.
        if self.local_address(slot).is_some() {
            return false;
        }
        let value = self.bcx.use_var(*var);
        known::known_zero(&self.bcx.func.dfg, value)
    }

    /// A release of each local that may hold a value. Exits whose cleanup
    /// drops different frame locals still release the same ones, which then
    /// hold nothing, so their lists share suffixes; a dropped variable is
    /// known empty once its drop is lowered, so it is omitted.
    fn push_releases(&mut self, steps: &mut Vec<Step>, cleanup: &[Op]) {
        let dropped: Vec<u8> = cleanup
            .iter()
            .filter_map(|op| match op {
                Op::DropLocal(i) => Some(*i),
                _ => None,
            })
            .collect();
        for i in (0..self.locals.len()).rev() {
            let slot = i as u8;
            let frame = self.local_address(slot).is_some();
            if (dropped.contains(&slot) && !frame) || self.known_empty(slot) {
                continue;
            }
            steps.push(Step::Release(slot));
        }
    }

    /// Jump into `which` ladder's chain for `steps`, carrying `carried` to
    /// the target `end`.
    fn take_exit(
        &mut self,
        which: Which,
        steps: Vec<Step>,
        restore: Option<crate::ownership::Site>,
        carried: Vec<NativeValue>,
        end: End,
    ) -> Result<()> {
        let types: Vec<Type> = carried
            .iter()
            .map(|v| self.bcx.func.dfg.value_type(*v))
            .collect();
        let mut exits = self.exits.take().expect("shared exits");
        let ladder = match which {
            Which::Function => &mut exits.function,
            Which::Loop => exits.loops.last_mut().expect("a loop's shared exits"),
        };
        let found = ladder
            .targets
            .iter()
            .position(|target| target.types == types && target.end.joins(&end));
        let (end, called) = match end {
            // The callee joins the target's below.
            End::Calls(called) => (End::Calls(Vec::new()), called.first().copied()),
            end => (end, None),
        };
        let target = match found {
            Some(i) => i,
            None => {
                let block = self.parameterized_block(&types);
                ladder.targets.push(Target {
                    block,
                    types: types.clone(),
                    end,
                });
                ladder.targets.len() - 1
            }
        };
        let arm = match (&mut ladder.targets[target].end, called) {
            (End::Calls(callees), Some(callee)) => {
                let known = callees.iter().position(|known| {
                    known.id == callee.id && known.track_caller == callee.track_caller
                });
                Some(known.unwrap_or_else(|| {
                    callees.push(callee);
                    callees.len() - 1
                }))
            }
            _ => None,
        };
        let mut next = Next::Target(target);
        for step in steps.into_iter().rev() {
            let found = ladder.entering.get(&next).and_then(|entering| {
                entering
                    .iter()
                    .copied()
                    .find(|&i| ladder.steps[i].step.same(&step))
            });
            next = Next::Step(match found {
                Some(i) => i,
                None => {
                    let block = self.parameterized_block(&types);
                    ladder.steps.push(Shared {
                        block,
                        carried: types.len(),
                        step,
                        next,
                    });
                    let i = ladder.steps.len() - 1;
                    ladder.entering.entry(next).or_default().push(i);
                    i
                }
            });
        }
        let target = ladder.block(next);
        let sites = *exits
            .sites
            .get_or_insert_with(|| (self.bcx.declare_var(PTR_TY), self.bcx.declare_var(PTR_TY)));
        // A target with one callee never reads the variable, so SSA
        // construction gives it no parameters and the constant goes unused.
        let calling = arm.map(|arm| {
            let var = *exits
                .callee
                .get_or_insert_with(|| self.bcx.declare_var(types::I32));
            (var, arm)
        });
        let restoring = restore.map(|site| {
            let vars = *exits.restore.get_or_insert_with(|| {
                (self.bcx.declare_var(PTR_TY), self.bcx.declare_var(PTR_TY))
            });
            (vars, site)
        });
        self.exits = Some(exits);
        self.pass_sites(sites)?;
        if let Some((var, arm)) = calling {
            let index = self.bcx.ins().iconst(types::I32, arm as i64);
            self.bcx.def_var(var, index);
        }
        if let Some(((trap, call), site)) = restoring {
            let at = self.site_record(Some((site.line, site.column)))?;
            self.bcx.def_var(trap, at);
            let caller = if self.in_standard_library() {
                self.bcx.ins().iconst(PTR_TY, 0)
            } else {
                at
            };
            self.bcx.def_var(call, caller);
        }
        let args: Vec<BlockArg> = carried.iter().map(|v| BlockArg::Value(*v)).collect();
        self.bcx.ins().jump(target, &args);
        Ok(())
    }

    fn parameterized_block(&mut self, types: &[Type]) -> Block {
        let block = self.bcx.create_block();
        for ty in types {
            self.bcx.append_block_param(block, *ty);
        }
        block
    }

    /// Share the `break`s and `continue`s of the loop about to be lowered,
    /// which exits to `exit` and continues at `header`.
    pub(super) fn enter_loop_exits(&mut self, header: Block, exit: Block) {
        if let Some(exits) = &mut self.exits {
            let target = |block| Target {
                block,
                types: Vec::new(),
                end: End::Jump(block),
            };
            exits.loops.push(Ladder {
                targets: vec![target(exit), target(header)],
                ..Default::default()
            });
        }
    }

    /// Lower the blocks shared by the `break`s and `continue`s of the loop
    /// just lowered, before its header and exit block are sealed.
    pub(super) fn finish_loop_exits(&mut self) -> Result<()> {
        let Some(mut exits) = self.exits.take() else {
            return Ok(());
        };
        let ladder = exits.loops.pop().expect("a loop's shared exits");
        let (sites, restore, callee) = (exits.sites, exits.restore, exits.callee);
        let pending = exits.pending.is_some();
        self.exits = Some(exits);
        if pending {
            return Err("AOT: scope cleanup without its exit".into());
        }
        let (Some(sites), false) = (sites, ladder.steps.is_empty()) else {
            return Ok(());
        };
        // The loop's exit block continues from the body's state.
        let stack = std::mem::take(&mut self.stack);
        let position = self.position.clone();
        let srcloc = self.bcx.srcloc();
        let exits = self.exits.take();
        let lowered = self.lower_ladder(ladder, sites, restore, callee, None);
        self.exits = exits;
        self.stack = stack;
        self.position = position;
        self.bcx.set_srcloc(srcloc);
        lowered
    }

    /// Lower the shared blocks once every exit has jumped to them.
    pub(super) fn finish_exits(&mut self) -> Result<()> {
        let Some(exits) = self.exits.take() else {
            return Ok(());
        };
        if exits.pending.is_some() {
            return Err("AOT: scope cleanup without its exit".into());
        }
        let Some(sites) = exits.sites else {
            return Ok(());
        };
        // The body's returned operands were passed to the blocks.
        self.stack.clear();
        self.lower_ladder(
            exits.function,
            sites,
            exits.restore,
            exits.callee,
            exits.returns_to,
        )?;
        self.terminated = true;
        Ok(())
    }

    fn lower_ladder(
        &mut self,
        ladder: Ladder,
        sites: (Variable, Variable),
        restore: Option<(Variable, Variable)>,
        callee: Option<Variable>,
        returns_to: Option<Block>,
    ) -> Result<()> {
        self.terminated = false;
        self.slot_copies.clear();
        self.fresh_load = None;
        // Every predecessor of a step is an exit or a later step.
        for shared in ladder.steps.iter().rev() {
            self.enter_shared(shared.block, sites);
            match &shared.step {
                Step::Ops(ops, range) => {
                    for op in &ops[range.clone()] {
                        self.lower(op)?;
                    }
                    if !self.stack.is_empty() {
                        return Err(format!(
                            "AOT: cleanup step left operands on the stack: {:?} after {:?}",
                            self.stack.iter().map(|(_, t)| t).collect::<Vec<_>>(),
                            &ops[range.clone()]
                        )
                        .into());
                    }
                }
                Step::Restore => {
                    let (trap, call) = restore.ok_or("AOT: restore step without its positions")?;
                    self.position.share(trap, call);
                    // The restored line differs between exits.
                    self.bcx
                        .set_srcloc(cranelift_codegen::ir::SourceLoc::default());
                }
                Step::Release(slot) => {
                    let ty = self.locals[*slot as usize].1.clone();
                    let value = self.read_local(*slot);
                    self.release(value, &ty);
                }
            }
            self.pass_sites(sites)?;
            let args: Vec<BlockArg> = self.bcx.block_params(shared.block)[..shared.carried]
                .iter()
                .map(|v| BlockArg::Value(*v))
                .collect();
            let next = ladder.block(shared.next);
            self.bcx.ins().jump(next, &args);
        }
        for target in &ladder.targets {
            if matches!(target.end, End::Jump(_)) {
                continue;
            }
            self.enter_shared(target.block, sites);
            let values = self.bcx.block_params(target.block)[..target.types.len()].to_vec();
            match &target.end {
                End::Return => self.return_to(returns_to, &values),
                End::Calls(callees) => self.dispatch(callees, callee, &values)?,
                End::CallIndirect(_, reference, track_caller) => {
                    if *track_caller {
                        self.enter_call()?;
                    }
                    self.bcx
                        .ins()
                        .return_call_indirect(*reference, values[0], &values[1..]);
                }
                End::Jump(_) => unreachable!(),
            }
        }
        Ok(())
    }

    /// Return `values`, or pass them to `returns_to` in an inlined body.
    fn return_to(&mut self, returns_to: Option<Block>, values: &[NativeValue]) {
        match returns_to {
            Some(block) => {
                let args: Vec<BlockArg> = values.iter().map(|v| BlockArg::Value(*v)).collect();
                self.bcx.ins().jump(block, &args);
            }
            None => {
                self.bcx.ins().return_(values);
            }
        }
    }

    /// Tail-call the callee `var` selects among `callees` with `values`.
    fn dispatch(
        &mut self,
        callees: &[Callee],
        var: Option<Variable>,
        values: &[NativeValue],
    ) -> Result<()> {
        let blocks: Vec<Block> = match callees {
            [] => return Err("AOT: tail-call target without a callee".into()),
            [_] => vec![self.bcx.current_block().expect("a target block")],
            _ => {
                let var = var.ok_or("AOT: tail-call target without its callee index")?;
                let index = self.bcx.use_var(var);
                let blocks: Vec<Block> = callees.iter().map(|_| self.bcx.create_block()).collect();
                // The last callee's index is past the table.
                let (&last, table) = blocks.split_last().expect("callees");
                let table = JumpTableData::new(
                    self.bcx.func.dfg.block_call(last, &[]),
                    &table
                        .iter()
                        .map(|&block| self.bcx.func.dfg.block_call(block, &[]))
                        .collect::<Vec<_>>(),
                );
                let table = self.bcx.create_jump_table(table);
                self.bcx.ins().br_table(index, table);
                blocks
            }
        };
        for (block, callee) in blocks.into_iter().zip(callees) {
            if self.bcx.current_block() != Some(block) {
                self.bcx.switch_to_block(block);
                self.bcx.seal_block(block);
            }
            if callee.track_caller {
                self.enter_call()?;
            }
            self.bcx.ins().return_call(callee.func, values);
        }
        Ok(())
    }

    /// Enter a shared block, which has no source line of its own.
    fn enter_shared(&mut self, block: Block, (trap, call): (Variable, Variable)) {
        self.bcx.switch_to_block(block);
        self.bcx.seal_block(block);
        self.bcx
            .set_srcloc(cranelift_codegen::ir::SourceLoc::default());
        self.position.share(trap, call);
    }

    /// Record the current position for the shared block about to be entered.
    fn pass_sites(&mut self, (trap, call): (Variable, Variable)) -> Result<()> {
        let at = self.trap_site()?;
        self.bcx.def_var(trap, at);
        // Standard-library code names no caller.
        let caller = if self.in_standard_library() {
            self.bcx.ins().iconst(PTR_TY, 0)
        } else {
            self.call_site()?
        };
        self.bcx.def_var(call, caller);
        Ok(())
    }

    fn follow_markers(&mut self, ops: &[Op]) {
        for op in ops {
            match op {
                Op::Site(site) => {
                    if self.module.lines() {
                        self.bcx
                            .set_srcloc(cranelift_codegen::ir::SourceLoc::new(site.line));
                    }
                    self.position.set(site);
                }
                Op::CallerSite(site) => self.position.set_call(site),
                Op::Match(arms) => arms.iter().for_each(|arm| self.follow_markers(&arm.body)),
                Op::Loop { condition, body } => {
                    self.follow_markers(condition);
                    self.follow_markers(body);
                }
                Op::If {
                    then,
                    elifs,
                    otherwise,
                } => {
                    self.follow_markers(then);
                    for arm in elifs.iter() {
                        self.follow_markers(&arm.condition);
                        self.follow_markers(&arm.body);
                    }
                    self.follow_markers(otherwise);
                }
                _ => {}
            }
        }
    }
}

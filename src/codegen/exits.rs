//! Shared exits of a user function (issues #407 and #417).
//!
//! A `?` failure runs its cleanup ops, then, like every `return` and tail
//! call, releases each owned local that may still hold a value and leaves.
//! A `return` or `break` inside scopes first runs the scope cleanup the front
//! end marks with [`Op::Leaving`]. Lowered at each exit, that code is
//! proportional to the live locals, so a function with *n* exits and *n* owned
//! locals grows quadratically. Instead each exit stages the values it carries
//! and jumps into a chain of shared blocks.
//!
//! An exit's work is a list of steps: its cleanup ops, split after each
//! `DropLocal`, then, unless it is a `break`, one release per local that may
//! hold a value, in reverse slot order. One block performs each distinct
//! (step, rest of the list) pair and jumps to the block for the rest; the last
//! jumps to the exit's target: a return, a tail call, or a loop's exit block.
//! Successive exits in a scope release the same values in the same order after
//! their newest locals, so their lists share suffixes and the chain is a
//! ladder entered at each exit's newest live value. Lists with no common
//! suffix still share identical ones.
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
//! - Shared blocks are lowered after every predecessor exists, and sealed
//!   first, so a local every predecessor leaves empty still reads as the empty
//!   constant (issue #391): a function's blocks after its body, a loop's
//!   before its exit block is sealed.
use super::*;
use cranelift_codegen::ir::{FuncRef, SigRef, Signature, Type, Value as NativeValue};
use std::ops::Range;

#[derive(Default)]
pub(super) struct Exits {
    /// Trap and caller records of the exit being taken.
    sites: Option<(Variable, Variable)>,
    /// Records of the position a [`Step::Restore`] returns to.
    restore: Option<(Variable, Variable)>,
    /// The scope cleanup gathered since an `Op::Leaving`, until its `Return`
    /// or `Break`.
    pending: Option<Vec<Op>>,
    /// Exits leaving the function.
    function: Ladder,
    /// `break`s of each enclosing loop, innermost last.
    loops: Vec<Ladder>,
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
    /// A loop's exit block, lowered by the loop.
    Jump,
    /// A tail call, and whether it names this function's caller. Each
    /// declaration of a callee makes a new reference, so the function's id
    /// identifies it.
    Call(FuncId, FuncRef, bool),
    /// The callee is the first carried value.
    CallIndirect(Signature, SigRef, bool),
}

impl PartialEq for End {
    fn eq(&self, other: &End) -> bool {
        match (self, other) {
            (End::Return, End::Return) | (End::Jump, End::Jump) => true,
            (End::Call(a, _, x), End::Call(b, _, y)) => a == b && x == y,
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
    /// Cleanup ops of a `?`, `return` or `break`.
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
            self.bcx.ins().return_(&returned);
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
        if !matches!(op, Op::Return | Op::Break) {
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
            self.leave_loop(&cleanup)?;
        }
        self.terminated = true;
        self.fresh_load = None;
        self.slot_copies.clear();
        Ok(true)
    }

    fn leave_loop(&mut self, cleanup: &Arc<[Op]>) -> Result<()> {
        let (steps, restore) = self.cleanup_steps(cleanup);
        if steps.is_empty() {
            let &(_, exit) = self
                .loop_targets
                .last()
                .ok_or("loop control outside a loop")?;
            self.bcx.ins().jump(exit, &[]);
        } else {
            self.take_exit(Which::Loop, steps, restore, Vec::new(), End::Jump)?;
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
        self.shared_tail(args.to_vec(), End::Call(id, func, track_caller))
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
            .position(|target| target.types == types && target.end == end);
        let mut next = Next::Target(match found {
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
        });
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
        let restoring = restore.map(|site| {
            let vars = *exits.restore.get_or_insert_with(|| {
                (self.bcx.declare_var(PTR_TY), self.bcx.declare_var(PTR_TY))
            });
            (vars, site)
        });
        self.exits = Some(exits);
        self.pass_sites(sites)?;
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

    /// Share the `break`s of the loop about to be lowered, which exits to
    /// `exit`.
    pub(super) fn enter_loop_exits(&mut self, exit: Block) {
        if let Some(exits) = &mut self.exits {
            exits.loops.push(Ladder {
                targets: vec![Target {
                    block: exit,
                    types: Vec::new(),
                    end: End::Jump,
                }],
                ..Default::default()
            });
        }
    }

    /// Lower the blocks shared by the `break`s of the loop just lowered,
    /// before its exit block is sealed.
    pub(super) fn finish_loop_exits(&mut self) -> Result<()> {
        let Some(mut exits) = self.exits.take() else {
            return Ok(());
        };
        let ladder = exits.loops.pop().expect("a loop's shared exits");
        let (sites, restore) = (exits.sites, exits.restore);
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
        let lowered = self.lower_ladder(ladder, sites, restore);
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
        self.lower_ladder(exits.function, sites, exits.restore)?;
        self.terminated = true;
        Ok(())
    }

    fn lower_ladder(
        &mut self,
        ladder: Ladder,
        sites: (Variable, Variable),
        restore: Option<(Variable, Variable)>,
    ) -> Result<()> {
        self.terminated = false;
        self.slot_copies.clear();
        self.fresh_load = None;
        self.bcx
            .set_srcloc(cranelift_codegen::ir::SourceLoc::default());
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
            if matches!(target.end, End::Jump) {
                continue;
            }
            self.enter_shared(target.block, sites);
            let values = self.bcx.block_params(target.block)[..target.types.len()].to_vec();
            match &target.end {
                End::Return => {
                    self.bcx.ins().return_(&values);
                }
                End::Call(_, func, track_caller) => {
                    if *track_caller {
                        self.enter_call()?;
                    }
                    self.bcx.ins().return_call(*func, &values);
                }
                End::CallIndirect(_, reference, track_caller) => {
                    if *track_caller {
                        self.enter_call()?;
                    }
                    self.bcx
                        .ins()
                        .return_call_indirect(*reference, values[0], &values[1..]);
                }
                End::Jump => unreachable!(),
            }
        }
        Ok(())
    }

    fn enter_shared(&mut self, block: Block, (trap, call): (Variable, Variable)) {
        self.bcx.switch_to_block(block);
        self.bcx.seal_block(block);
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

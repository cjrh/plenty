//! Shared exits of a user function (issue #407).
//!
//! A `?` failure runs its cleanup ops, then, like every `return`, releases
//! each owned local that may still hold a value and returns. Lowered at each
//! exit, that code is proportional to the live locals, so a function with *n*
//! exits and *n* owned locals grows quadratically. Instead each exit stages
//! its returned values and jumps into a chain of shared blocks.
//!
//! An exit's work is a list of steps: its cleanup ops, split after each
//! `DropLocal`, then one release per local that may hold a value, in reverse
//! slot order. One block performs each distinct (step, rest of the list) pair
//! and jumps to the block for the rest; the last jumps to a return block.
//! Successive exits in a scope release the same values in the same order after
//! their newest locals, so their lists share suffixes and the chain is a
//! ladder entered at each exit's newest live value. Lists with no common
//! suffix still share identical ones.
//!
//! Behavior matches lowering each exit inline:
//! - Steps run in the same order, from the same operand stack (empty). The
//!   returned values are staged first; nothing a step does can reach them.
//! - An exit omits the release of a local whose value it knows is empty, as
//!   [`Lowerer::release`] would, and of one its cleanup drops.
//! - A shared block cannot name one source position, so exits pass their
//!   trap and caller records in two variables, which the blocks read until a
//!   marker of their own fixes the position ([`sites::Position::share`]).
//! - Shared blocks are lowered after the body, once all their predecessors
//!   exist, and sealed first, so a local every predecessor leaves empty still
//!   reads as the empty constant (issue #391).
use super::*;
use cranelift_codegen::ir::Type;
use std::ops::Range;

#[derive(Default)]
pub(super) struct Exits {
    /// Trap and caller records of the exit being taken.
    sites: Option<(Variable, Variable)>,
    /// One return block per list of returned value types.
    returns: Vec<(Vec<Type>, Block)>,
    /// Each step follows the step or return it jumps to.
    steps: Vec<Shared>,
    /// The steps jumping to each target.
    entering: HashMap<Next, Vec<usize>>,
}

struct Shared {
    block: Block,
    /// How many of the block's leading parameters are returned values; SSA
    /// construction appends more for the variables the block reads.
    returned: usize,
    step: Step,
    next: Next,
}

enum Step {
    /// Cleanup ops of a `?`.
    Ops(Arc<[Op]>, Range<usize>),
    /// Release a local without emptying it: the function is returning.
    Release(u8),
}

impl Step {
    fn same(&self, other: &Step) -> bool {
        match (self, other) {
            (Step::Ops(a, x), Step::Ops(b, y)) => a[x.clone()] == b[y.clone()],
            (Step::Release(a), Step::Release(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Next {
    Step(usize),
    Return(usize),
}

impl Exits {
    fn block(&self, next: Next) -> Block {
        match next {
            Next::Step(i) => self.steps[i].block,
            Next::Return(i) => self.returns[i].1,
        }
    }
}

/// `cleanup` split after each `DropLocal`. The front end emits each
/// context's receiver and exit together before the local's drop, so the
/// operand stack is empty at every boundary.
fn chunks(cleanup: &[Op]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (i, op) in cleanup.iter().enumerate() {
        if matches!(op, Op::DropLocal(_)) {
            ranges.push(start..i + 1);
            start = i + 1;
        }
    }
    if start < cleanup.len() {
        ranges.push(start..cleanup.len());
    }
    ranges
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
        let mut steps: Vec<Step> = chunks(cleanup)
            .into_iter()
            .map(|range| Step::Ops(cleanup.clone(), range))
            .collect();
        let dropped: Vec<u8> = cleanup
            .iter()
            .filter_map(|op| match op {
                Op::DropLocal(i) => Some(*i),
                _ => None,
            })
            .collect();
        for i in (0..self.locals.len()).rev() {
            let (var, ty) = &self.locals[i];
            let slot = i as u8;
            if !ty.managed() || dropped.contains(&slot) {
                continue;
            }
            // Reading a variable here emits nothing; a frame load would.
            if self.local_address(slot).is_none() {
                let value = self.bcx.use_var(*var);
                if known::known_zero(&self.bcx.func.dfg, value) {
                    continue;
                }
            }
            steps.push(Step::Release(slot));
        }
        // With nothing to share, a jump would only replace the return.
        if steps.is_empty() {
            self.bcx.ins().return_(&returned);
            self.follow_markers(cleanup);
            return Ok(());
        }
        let types: Vec<Type> = returned
            .iter()
            .map(|v| self.bcx.func.dfg.value_type(*v))
            .collect();
        let mut exits = self.exits.take().expect("shared exits");
        let mut next = Next::Return(match exits.returns.iter().position(|(t, _)| *t == types) {
            Some(i) => i,
            None => {
                let block = self.parameterized_block(&types);
                exits.returns.push((types.clone(), block));
                exits.returns.len() - 1
            }
        });
        for step in steps.into_iter().rev() {
            let found = exits.entering.get(&next).and_then(|entering| {
                entering
                    .iter()
                    .copied()
                    .find(|&i| exits.steps[i].step.same(&step))
            });
            next = Next::Step(match found {
                Some(i) => i,
                None => {
                    let block = self.parameterized_block(&types);
                    exits.steps.push(Shared {
                        block,
                        returned: types.len(),
                        step,
                        next,
                    });
                    let i = exits.steps.len() - 1;
                    exits.entering.entry(next).or_default().push(i);
                    i
                }
            });
        }
        let target = exits.block(next);
        let sites = *exits
            .sites
            .get_or_insert_with(|| (self.bcx.declare_var(PTR_TY), self.bcx.declare_var(PTR_TY)));
        self.exits = Some(exits);
        self.pass_sites(sites)?;
        let args: Vec<BlockArg> = returned.iter().map(|v| BlockArg::Value(*v)).collect();
        self.bcx.ins().jump(target, &args);
        // Inline lowering would leave the position at cleanup's last marker.
        self.follow_markers(cleanup);
        Ok(())
    }

    fn parameterized_block(&mut self, types: &[Type]) -> Block {
        let block = self.bcx.create_block();
        for ty in types {
            self.bcx.append_block_param(block, *ty);
        }
        block
    }

    /// Lower the shared blocks once every exit has jumped to them.
    pub(super) fn finish_exits(&mut self) -> Result<()> {
        let Some(exits) = self.exits.take() else {
            return Ok(());
        };
        let Some(sites) = exits.sites else {
            return Ok(());
        };
        // The body's returned operands were passed to the blocks.
        self.stack.clear();
        self.terminated = false;
        self.slot_copies.clear();
        self.fresh_load = None;
        self.bcx
            .set_srcloc(cranelift_codegen::ir::SourceLoc::default());
        // Every predecessor of a step is an exit or a later step.
        for shared in exits.steps.iter().rev() {
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
                Step::Release(slot) => {
                    let ty = self.locals[*slot as usize].1.clone();
                    let value = self.read_local(*slot);
                    self.release(value, &ty);
                }
            }
            self.pass_sites(sites)?;
            let args: Vec<BlockArg> = self.bcx.block_params(shared.block)[..shared.returned]
                .iter()
                .map(|v| BlockArg::Value(*v))
                .collect();
            let next = exits.block(shared.next);
            self.bcx.ins().jump(next, &args);
        }
        for (types, block) in &exits.returns {
            self.enter_shared(*block, sites);
            let values = self.bcx.block_params(*block)[..types.len()].to_vec();
            self.bcx.ins().return_(&values);
        }
        self.terminated = true;
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

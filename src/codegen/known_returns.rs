//! A return of an inlined call whose `Result` or `Option` is known to hold
//! the success variant skips the caller's `?` test (issue #936). See
//! `book/src/design/toolchain/02-architecture/emission.md#known-returns`.
//!
//! An inlined callee's returns jump to one block that takes the returned
//! value, and `?` there tests the value's tag. Cranelift threads no jumps, so
//! a return that built `Ok(x)` still ran the test. Here `?` moves each such
//! jump to its success block, passing the payload word.
//!
//! Moving a jump needs its old target unsealed: the frontend rejects removing
//! a predecessor of a sealed block. The return block of an inlined call and
//! the exit of an inlined `checked` function body therefore stay unsealed
//! until the next op that is not `?`. Sealing late changes no generated code.
use super::*;
use cranelift_codegen::ir::{
    DataFlowGraph, Inst, InstructionData, Opcode, Value as NativeValue, ValueDef,
};

#[derive(Default)]
pub(super) struct Joins {
    /// The jumps into each block a `?` may bypass, until the block is sealed.
    jumps: HashMap<Block, Vec<Inst>>,
    /// Blocks awaiting [`Lowerer::seal_joins`].
    unsealed: Vec<Block>,
}

impl Joins {
    pub(super) fn pending(&self) -> bool {
        !self.unsealed.is_empty()
    }
}

/// How the recorded jumps reach a `?`.
#[derive(Default)]
struct Arrivals {
    /// Jumps whose value holds the success variant: the jump, its target and
    /// the payload's low word.
    ready: Vec<(Inst, Block, NativeValue)>,
    /// Blocks holding only a jump that passes their parameter on.
    forwarding: Vec<Block>,
    /// Some arrival's variant is unknown.
    unknown: bool,
}

/// How many forwarding blocks a `?` looks through: a `checked` function's
/// exit and the return blocks of nested inlined calls.
const DEPTH: u8 = 3;

/// The low word and tag of a sum value built in view.
fn built(dfg: &DataFlowGraph, value: NativeValue) -> Option<(NativeValue, u64)> {
    let ValueDef::Result(inst, _) = dfg.value_def(dfg.resolve_aliases(value)) else {
        return None;
    };
    match dfg.insts[inst] {
        InstructionData::Unary {
            opcode: Opcode::Uextend,
            arg,
        } if dfg.value_type(arg) == types::I64 => Some((arg, 0)),
        InstructionData::Binary {
            opcode: Opcode::Iconcat,
            args: [low, high],
        } => Some((low, known::known_int(dfg, high)? & 1)),
        _ => None,
    }
}

impl Lowerer<'_, '_> {
    /// Record `jump` passing one sum value to `block`.
    pub(super) fn note_join(&mut self, block: Block, jump: Inst) {
        self.joins.jumps.entry(block).or_default().push(jump);
    }

    /// Leave `block` unsealed for a `?` that may follow, when jumps into it
    /// were recorded; otherwise seal it.
    pub(super) fn seal_join_later(&mut self, block: Block, keep: bool) {
        if keep && self.joins.jumps.contains_key(&block) {
            self.joins.unsealed.push(block);
        } else {
            self.joins.jumps.remove(&block);
            self.bcx.seal_block(block);
        }
    }

    pub(super) fn seal_joins(&mut self) {
        for block in std::mem::take(&mut self.joins.unsealed) {
            self.joins.jumps.remove(&block);
            self.bcx.seal_block(block);
        }
    }

    /// The success block of a `?` on `value`, when returns known to hold
    /// `success_tag` now jump to it. It takes the payload's low word, which
    /// is the whole payload of `ty`. The flag tells that every other arrival
    /// is known to hold the failure variant, so the `?` needs no test: every
    /// jump into these blocks is recorded, by `return_to` and by the two
    /// exits of a `checked` body.
    pub(super) fn bypass_try(
        &mut self,
        value: NativeValue,
        success_tag: u64,
        ty: &Ty,
        failure: Block,
    ) -> Option<(Block, bool)> {
        if !self.joins.pending() {
            return None;
        }
        let storage = ty.quantity_storage().unwrap_or_else(|| ty.clone());
        let one_word = !storage.wide()
            && !matches!(storage, Ty::Ref(..) | Ty::View(..))
            && clif_type(storage).bits() <= 64;
        let current = self.bcx.current_block()?;
        let mut arrivals = Arrivals::default();
        // Nothing lowered since the call: the value is still only the
        // block's parameter.
        if self.bcx.func.layout.first_inst(current).is_some() {
            return None;
        }
        if !one_word || !self.arrivals(current, value, success_tag, DEPTH, &mut arrivals) {
            return None;
        }
        if arrivals.ready.is_empty() {
            return None;
        }
        let success = self.bcx.create_block();
        self.bcx.append_block_param(success, types::I64);
        for &(jump, target, low) in &arrivals.ready {
            self.bcx.change_jump_destination(jump, target, success);
            let dfg = &mut self.bcx.func.dfg;
            let InstructionData::Jump { destination, .. } = &mut dfg.insts[jump] else {
                unreachable!("an arrival is a jump")
            };
            destination.update_args(&mut dfg.value_lists, |_| BlockArg::Value(low));
        }
        if !arrivals.unknown {
            for block in arrivals.forwarding {
                self.bcx.set_cold_block(block);
            }
            self.bcx.set_cold_block(current);
            self.bcx.set_cold_block(failure);
        }
        Some((success, !arrivals.unknown))
    }

    /// Gather the recorded jumps into `block`, an unsealed block that holds
    /// nothing before the position being lowered and whose only parameter is
    /// `param`. False when it is not such a block.
    fn arrivals(
        &self,
        block: Block,
        param: NativeValue,
        success_tag: u64,
        depth: u8,
        arrivals: &mut Arrivals,
    ) -> bool {
        let func = &self.bcx.func;
        let Some(jumps) = self.joins.jumps.get(&block) else {
            return false;
        };
        if !self.joins.unsealed.contains(&block) || func.dfg.block_params(block) != [param] {
            return false;
        }
        for &jump in jumps {
            let InstructionData::Jump { destination, .. } = func.dfg.insts[jump] else {
                arrivals.unknown = true;
                continue;
            };
            let Some(from) = func.layout.inst_block(jump) else {
                continue;
            };
            let mut args = destination.args(&func.dfg.value_lists);
            let (Some(BlockArg::Value(passed)), None) = (args.next(), args.next()) else {
                arrivals.unknown = true;
                continue;
            };
            if destination.block(&func.dfg.value_lists) != block {
                arrivals.unknown = true;
                continue;
            }
            match built(&func.dfg, passed) {
                Some((low, tag)) if tag == success_tag => arrivals.ready.push((jump, block, low)),
                Some(_) => {}
                None if depth > 0
                    && func.layout.first_inst(from) == Some(jump)
                    && self.arrivals(from, passed, success_tag, depth - 1, arrivals) =>
                {
                    arrivals.forwarding.push(from)
                }
                None => arrivals.unknown = true,
            }
        }
        true
    }
}

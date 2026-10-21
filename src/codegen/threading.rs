//! Branch threading through the join blocks of `and`, `or`, and other
//! boolean matches (issue #377).
//!
//! `a or b` lowers to a `match` on `a` whose arms push `True` or evaluate `b`,
//! and the arms meet in a join block that receives the `bool` as a parameter.
//! When an `if`, `while`, `match`, or enclosing `and`/`or` consumes the result,
//! the join block only branches on that parameter, often on an edge where an
//! arm passed a constant. Cranelift does not thread branches through block
//! parameters, so without this pass each operand of a chain adds a constant
//! phi and a re-test to release code.
//!
//! [`thread_branches`] retargets an edge into such a *pass-through* block to
//! the block it would reach: a constant decides the pass-through block's
//! `brif`, and a `jump` into it whose decision is not known takes over the
//! `brif` itself. The pass moves only branches. Every operand still runs in
//! its own block in source order, so evaluation order, laziness, traps, and
//! `?` propagation are unchanged.
use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::entity::EntitySet;
use cranelift_codegen::ir::{
    Block, BlockArg, BlockCall, Function, Inst, InstBuilder, InstructionData, Opcode, Type, Value,
    ValueDef, types,
};

use super::known::{known_int, known_int_given};

/// Bounds how many pass-through blocks one edge skips per round. A chain of
/// `n` operands needs about two steps per operand.
const STEPS: u32 = 32;
/// Bounds the rounds over the function. A round rarely changes anything
/// after the first; the bound also stops a constant infinite loop, whose
/// edges would otherwise be retargeted around it forever.
const ROUNDS: u32 = 4;

/// An argument for a retargeted edge: an existing value, or a constant the
/// skipped block computed, materialized in the edge's block on commit.
#[derive(Clone, Copy, PartialEq)]
enum Arg {
    Value(Value),
    Const(Type, u64),
}

/// An edge under construction: its target and arguments.
#[derive(Clone, PartialEq)]
struct Edge {
    block: Block,
    args: Vec<Arg>,
}

/// What an edge into a pass-through block becomes.
enum Next {
    /// The block's `jump`, or its `brif` decided by the edge (`true`).
    Jump(Edge, bool),
    /// The block's `brif` on a parameter whose argument is not known.
    Branch(Value, Edge, Edge),
}

/// Retargets edges into pass-through blocks; see the module comment.
/// Blocks left unreachable are removed by Cranelift before it optimizes.
pub(super) fn thread_branches(func: &mut Function) {
    let Some(candidates) = candidates(func) else {
        return;
    };
    for _ in 0..ROUNDS {
        let mut changed = false;
        let mut block = func.layout.entry_block();
        while let Some(current) = block {
            if let Some(inst) = func.layout.last_inst(current) {
                changed |= thread_terminator(func, &candidates, inst);
            }
            block = func.layout.next_block(current);
        }
        if !changed {
            break;
        }
    }
}

/// Blocks an edge may skip: each ends in a `jump` or `brif`, has no other
/// instruction that traps, touches memory, or calls, and defines no value used
/// in another block, which would lose its definition on the new path. `None`
/// when no candidate has a `brif` to decide, so nothing could change.
///
/// Retargeting changes only terminators between `jump` and `brif`, inserts
/// constants, and never adds a use of a value outside its defining block that
/// was not already outside it, so the set stays valid across rounds.
fn candidates(func: &Function) -> Option<EntitySet<Block>> {
    let mut candidates = EntitySet::new();
    let mut branches = false;
    for block in func.layout.blocks() {
        let Some(terminator) = func.layout.last_inst(block) else {
            continue;
        };
        let branch = match func.dfg.insts[terminator].opcode() {
            Opcode::Brif => true,
            Opcode::Jump => false,
            _ => continue,
        };
        if func
            .layout
            .block_insts(block)
            .all(|inst| inst == terminator || pure(func.dfg.insts[inst].opcode()))
        {
            candidates.insert(block);
            branches |= branch;
        }
    }
    if !branches {
        return None;
    }
    for block in func.layout.blocks() {
        for inst in func.layout.block_insts(block) {
            for value in func.dfg.inst_values(inst) {
                if let Some(defined) = defining_block(func, value)
                    && defined != block
                {
                    candidates.remove(defined);
                }
            }
        }
    }
    Some(candidates)
}

fn defining_block(func: &Function, value: Value) -> Option<Block> {
    match func.dfg.value_def(func.dfg.resolve_aliases(value)) {
        ValueDef::Param(block, _) => Some(block),
        ValueDef::Result(inst, _) => func.layout.inst_block(inst),
        _ => None,
    }
}

/// Threads the edges of the terminator `inst`; true if any changed.
fn thread_terminator(func: &mut Function, candidates: &EntitySet<Block>, inst: Inst) -> bool {
    match func.dfg.insts[inst] {
        InstructionData::Jump { destination, .. } => {
            let Some(start) = edge(func, candidates, destination) else {
                return false;
            };
            func.dfg.insts[inst] = match walk(func, candidates, start, true) {
                Walk::Unchanged => return false,
                Walk::Jump(target) => InstructionData::Jump {
                    opcode: Opcode::Jump,
                    destination: commit(func, inst, &target),
                },
                Walk::Branch(condition, yes, no) => InstructionData::Brif {
                    opcode: Opcode::Brif,
                    arg: condition,
                    blocks: [commit(func, inst, &yes), commit(func, inst, &no)],
                },
            };
            true
        }
        InstructionData::Brif { arg, blocks, .. } => {
            let mut threaded = blocks;
            for call in &mut threaded {
                if let Some(start) = edge(func, candidates, *call)
                    && let Walk::Jump(target) = walk(func, candidates, start, false)
                {
                    *call = commit(func, inst, &target);
                }
            }
            if threaded == blocks {
                return false;
            }
            func.dfg.insts[inst] = InstructionData::Brif {
                opcode: Opcode::Brif,
                arg,
                blocks: threaded,
            };
            true
        }
        _ => false,
    }
}

/// Where an edge ends up after skipping pass-through blocks.
enum Walk {
    /// No `brif` was decided on the way: skipping plain jumps gains nothing
    /// the backend's branch simplification does not already do.
    Unchanged,
    Jump(Edge),
    Branch(Value, Edge, Edge),
}

/// Follows `start` through pass-through blocks. Only the terminator of a
/// `jump` can `adopt` a `brif` it cannot decide.
fn walk(func: &Function, candidates: &EntitySet<Block>, start: Edge, adopt: bool) -> Walk {
    let mut current = start;
    let mut decided = false;
    for _ in 0..STEPS {
        match step(func, candidates, &current) {
            Some(Next::Jump(next, decides)) if next != current => {
                decided |= decides;
                current = next;
            }
            Some(Next::Branch(condition, yes, no)) if adopt => {
                return Walk::Branch(condition, yes, no);
            }
            _ => break,
        }
    }
    if decided {
        Walk::Jump(current)
    } else {
        Walk::Unchanged
    }
}

/// The edge `call` names, or `None` if its target is not a candidate or an
/// argument is not a plain value.
fn edge(func: &Function, candidates: &EntitySet<Block>, call: BlockCall) -> Option<Edge> {
    if !candidates.contains(call.block(&func.dfg.value_lists)) {
        return None;
    }
    let args = call
        .args(&func.dfg.value_lists)
        .map(|arg| arg.as_value().map(Arg::Value))
        .collect::<Option<Vec<_>>>()?;
    Some(Edge {
        block: call.block(&func.dfg.value_lists),
        args,
    })
}

/// Where `edge` goes after its target, if the target is a pass-through block:
/// its values are used only inside it, its instructions before the terminator
/// are pure, and its `jump` or `brif` is decided, or forwarded, by the edge's
/// arguments.
fn step(func: &Function, candidates: &EntitySet<Block>, edge: &Edge) -> Option<Next> {
    let block = edge.block;
    if !candidates.contains(block) {
        return None;
    }
    let terminator = func.layout.last_inst(block)?;
    let params = func.dfg.block_params(block);
    let known = |value: Value| known_arg(func, *edge.args.get(param_index(params, value)?)?);
    let translate = |value: Value| -> Option<Arg> {
        let value = func.dfg.resolve_aliases(value);
        if let Some(index) = param_index(params, value) {
            return edge.args.get(index).copied();
        }
        if defining_block(func, value) != Some(block) {
            return Some(Arg::Value(value));
        }
        let bits = known_int_given(&func.dfg, value, &known)?;
        Some(Arg::Const(func.dfg.value_type(value), bits))
    };
    let target = |call: BlockCall| -> Option<Edge> {
        let args = call
            .args(&func.dfg.value_lists)
            .map(|arg| translate(arg.as_value()?))
            .collect::<Option<Vec<_>>>()?;
        Some(Edge {
            block: call.block(&func.dfg.value_lists),
            args,
        })
    };
    match func.dfg.insts[terminator] {
        InstructionData::Jump { destination, .. } => Some(Next::Jump(target(destination)?, false)),
        InstructionData::Brif { arg, blocks, .. } => {
            if let Some(condition) = known_int_given(&func.dfg, arg, &known) {
                return Some(Next::Jump(
                    target(blocks[usize::from(condition == 0)])?,
                    true,
                ));
            }
            // Taking over the `brif` removes the parameter, so only a `brif`
            // on one, or on its negation, qualifies.
            let (tested, negated) = negation(func, block, arg);
            param_index(params, tested)?;
            let Some(Arg::Value(condition)) = translate(tested) else {
                return None;
            };
            let (yes, no) = (target(blocks[0])?, target(blocks[1])?);
            Some(if negated {
                Next::Branch(condition, no, yes)
            } else {
                Next::Branch(condition, yes, no)
            })
        }
        _ => None,
    }
}

/// `(x, true)` when `value` is `not x` computed in `block`, else
/// `(value, false)`. `not` is `bxor x, 1` and a `bool` holds 0 or 1, so
/// branching on `x` with the targets swapped is the same test.
fn negation(func: &Function, block: Block, value: Value) -> (Value, bool) {
    let value = func.dfg.resolve_aliases(value);
    if let ValueDef::Result(inst, _) = func.dfg.value_def(value)
        && func.layout.inst_block(inst) == Some(block)
        && let InstructionData::Binary {
            opcode: Opcode::Bxor,
            args: [x, one],
        } = func.dfg.insts[inst]
        && func.dfg.value_type(x) == types::I8
        && known_int(&func.dfg, one) == Some(1)
    {
        return (func.dfg.resolve_aliases(x), true);
    }
    (value, false)
}

fn param_index(params: &[Value], value: Value) -> Option<usize> {
    params.iter().position(|&param| param == value)
}

fn known_arg(func: &Function, arg: Arg) -> Option<u64> {
    match arg {
        Arg::Value(value) => known_int(&func.dfg, value),
        Arg::Const(_, bits) => Some(bits),
    }
}

/// Instructions a skipped block may contain: no trap, memory access, call,
/// or other effect is lost when an edge no longer runs them.
fn pure(opcode: Opcode) -> bool {
    !(opcode.can_trap()
        || opcode.can_load()
        || opcode.can_store()
        || opcode.is_call()
        || opcode.is_terminator()
        || opcode.other_side_effects())
}

/// Builds `edge` as a block call of `inst`, materializing its constants
/// just before `inst`.
fn commit(func: &mut Function, inst: Inst, edge: &Edge) -> BlockCall {
    let mut args = Vec::with_capacity(edge.args.len());
    for arg in &edge.args {
        let value = match *arg {
            Arg::Value(value) => value,
            Arg::Const(ty, bits) => FuncCursor::new(func)
                .at_inst(inst)
                .ins()
                .iconst(ty, bits as i64),
        };
        args.push(BlockArg::Value(value));
    }
    func.dfg.block_call(edge.block, &args)
}

#[cfg(test)]
mod tests;

//! Removes the release of an ownership slot that a stack frame holds as
//! zero on every path (issue #549).
//!
//! A local whose address is taken lives in a stack slot, so lowering reads
//! it with a `load` and cannot tell an empty slot from an owned value.
//! [`super::Lowerer::release`] guards each release with a zero test that
//! jumps around the runtime call and its caller bookkeeping. This pass
//! decides those tests for slots whose address never leaves the function's
//! own loads and stores: when every path stores zero to the bytes a test
//! reads, the test becomes a `jump` past the release. Cranelift removes the
//! release block as unreachable, as for [`super::known::fold_constant_branches`].
//!
//! Cranelift's alias analysis forwards such stores to loads too, but only
//! after unreachable blocks are removed, and it treats every call as writing
//! all memory, so a release after a call would keep its runtime call.
use std::collections::HashMap;

use cranelift_codegen::entity::{EntitySet, SecondaryMap};
use cranelift_codegen::flowgraph::ControlFlowGraph;
use cranelift_codegen::ir::{
    Block, ExternalName, FuncRef, Function, Inst, InstructionData, Opcode, StackSlot,
    UserExternalName, Value, ValueDef,
};
use cranelift_module::FuncId;

use super::known::{known_int, known_signed};

/// Bytes `[offset, offset + size)` of a stack slot, all known zero.
type Range = (StackSlot, i64, i64);

/// Replaces each `brif` on a value loaded from known-zero stack bytes with a
/// `jump` to its zero target, and removes each call to `release` (the
/// runtime's `plenty_release`) whose argument is such a value.
pub(super) fn remove_empty_releases(func: &mut Function, release: Option<FuncId>) {
    if func.sized_stack_slots.is_empty() {
        return;
    }
    let addresses = slot_addresses(func);
    if addresses.is_empty() {
        return;
    }
    let release = release.map(|id| UserExternalName::new(0, id.as_u32()));
    let releases: Vec<FuncRef> = func
        .dfg
        .ext_funcs
        .iter()
        .filter(|(_, data)| {
            matches!(data.name, ExternalName::User(name)
                if Some(&func.params.user_named_funcs()[name]) == release.as_ref())
        })
        .map(|(func_ref, _)| func_ref)
        .collect();
    let cfg = ControlFlowGraph::with_function(func);
    let order = reverse_postorder(func, &cfg);
    // `None`: not yet reached, the identity of the meet.
    let mut entry: SecondaryMap<Block, Option<Vec<Range>>> = SecondaryMap::new();
    if let Some(&first) = order.first() {
        entry[first] = Some(Vec::new());
    }
    let mut changed = true;
    while changed {
        changed = false;
        for &block in &order {
            let Some(state) = entry[block].clone() else {
                continue;
            };
            let exit = transfer(func, &addresses, &releases, block, state, &mut Vec::new());
            for succ in cfg.succ_iter(block) {
                let merged = match &entry[succ] {
                    None => exit.clone(),
                    Some(old) => old.iter().filter(|r| exit.contains(r)).copied().collect(),
                };
                if entry[succ].as_ref() != Some(&merged) {
                    entry[succ] = Some(merged);
                    changed = true;
                }
            }
        }
    }
    let mut empty = Vec::new();
    for &block in &order {
        if let Some(state) = entry[block].clone() {
            transfer(func, &addresses, &releases, block, state, &mut empty);
        }
    }
    for inst in empty {
        match func.dfg.insts[inst] {
            InstructionData::Brif { blocks, .. } => {
                func.dfg.insts[inst] = InstructionData::Jump {
                    opcode: Opcode::Jump,
                    destination: blocks[1],
                };
            }
            _ => func.layout.remove_inst(inst),
        }
    }
}

/// The stack slot and byte offset each address value names, for slots whose
/// address is only used to load from or store to that slot. An address that
/// is stored, passed to a call or a block, or offset by an unknown amount
/// lets other code write the slot, so that slot is left out.
fn slot_addresses(func: &Function) -> HashMap<Value, (StackSlot, i64)> {
    let mut addresses = HashMap::new();
    let mut escaped = EntitySet::<StackSlot>::new();
    // Definitions precede uses in the layout except across blocks placed out
    // of dominance order; a later round picks those up.
    let mut grew = true;
    while grew {
        grew = false;
        for block in func.layout.blocks() {
            for inst in func.layout.block_insts(block) {
                let Some(&result) = func.dfg.inst_results(inst).first() else {
                    continue;
                };
                if addresses.contains_key(&result) {
                    continue;
                }
                let derived = match func.dfg.insts[inst] {
                    InstructionData::StackAddr {
                        opcode: Opcode::StackAddr,
                        stack_slot,
                        offset,
                    } => Some((stack_slot, i64::from(offset))),
                    InstructionData::Unary {
                        opcode: Opcode::Uextend | Opcode::Ireduce,
                        arg,
                    } => addresses.get(&func.dfg.resolve_aliases(arg)).copied(),
                    InstructionData::Binary {
                        opcode: Opcode::Iadd,
                        args: [a, b],
                    } => {
                        let offset = |base: Value, by: Value| {
                            let (slot, offset) = *addresses.get(&func.dfg.resolve_aliases(base))?;
                            Some((slot, offset + known_signed(&func.dfg, by)?))
                        };
                        offset(a, b).or_else(|| offset(b, a))
                    }
                    _ => None,
                };
                if let Some(derived) = derived {
                    addresses.insert(result, derived);
                    grew = true;
                }
            }
        }
    }
    for block in func.layout.blocks() {
        for inst in func.layout.block_insts(block) {
            let produces_address = func
                .dfg
                .inst_results(inst)
                .first()
                .is_some_and(|result| addresses.contains_key(result));
            let allowed: &[Value] = match &func.dfg.insts[inst] {
                InstructionData::Load { arg, .. } => std::slice::from_ref(arg),
                InstructionData::Store { args, .. } => std::slice::from_ref(&args[1]),
                _ if produces_address => func.dfg.inst_args(inst),
                _ => &[],
            };
            // An address stored to memory escapes even when it is also the
            // store's own address.
            let stored = match func.dfg.insts[inst] {
                InstructionData::Store { args, .. } => Some(args[0]),
                _ => None,
            };
            for value in func.dfg.inst_values(inst) {
                if (Some(value) == stored || !allowed.contains(&value))
                    && let Some(&(slot, _)) = addresses.get(&func.dfg.resolve_aliases(value))
                {
                    escaped.insert(slot);
                }
            }
        }
    }
    addresses.retain(|_, (slot, _)| !escaped.contains(*slot));
    addresses
}

/// The known-zero ranges after `block` given those at its entry. Adds to
/// `empty` the block's final `brif` if it tests a known-zero value, and each
/// call to one of `releases` with a known-zero argument.
fn transfer(
    func: &Function,
    addresses: &HashMap<Value, (StackSlot, i64)>,
    releases: &[FuncRef],
    block: Block,
    mut state: Vec<Range>,
    empty: &mut Vec<Inst>,
) -> Vec<Range> {
    let dfg = &func.dfg;
    // Values computed in this block from known-zero loads.
    let mut zeros: Vec<Value> = Vec::new();
    let is_zero = |zeros: &[Value], value: Value| {
        let value = dfg.resolve_aliases(value);
        zeros.contains(&value) || known_int(dfg, value) == Some(0) || {
            // `known_int` reads no two-word value; an empty two-word slot is a
            // zero-extended zero word.
            matches!(dfg.value_def(value), ValueDef::Result(inst, _)
                if matches!(dfg.insts[inst], InstructionData::Unary {
                    opcode: Opcode::Uextend, arg } if known_int(dfg, arg) == Some(0)))
        }
    };
    for inst in func.layout.block_insts(block) {
        match dfg.insts[inst] {
            InstructionData::Store {
                opcode,
                args: [value, address],
                offset,
                ..
            } => {
                let Some(&(slot, base)) = addresses.get(&dfg.resolve_aliases(address)) else {
                    continue;
                };
                let start = base + i64::from(offset);
                let size = match opcode {
                    Opcode::Istore8 => 1,
                    Opcode::Istore16 => 2,
                    Opcode::Istore32 => 4,
                    _ => i64::from(dfg.value_type(value).bytes()),
                };
                state.retain(|&(s, from, len)| {
                    s != slot || from + len <= start || start + size <= from
                });
                if is_zero(&zeros, value) {
                    state.push((slot, start, size));
                }
            }
            InstructionData::Load {
                opcode,
                arg,
                offset,
                ..
            } => {
                let Some(&(slot, base)) = addresses.get(&dfg.resolve_aliases(arg)) else {
                    continue;
                };
                let start = base + i64::from(offset);
                let size = match opcode {
                    Opcode::Load => i64::from(dfg.value_type(dfg.first_result(inst)).bytes()),
                    Opcode::Uload8 | Opcode::Sload8 => 1,
                    Opcode::Uload16 | Opcode::Sload16 => 2,
                    Opcode::Uload32 | Opcode::Sload32 => 4,
                    _ => continue,
                };
                if state
                    .iter()
                    .any(|&(s, from, len)| s == slot && from <= start && start + size <= from + len)
                {
                    zeros.push(dfg.first_result(inst));
                }
            }
            InstructionData::Unary {
                opcode: Opcode::Uextend | Opcode::Sextend | Opcode::Ireduce,
                arg,
            } if is_zero(&zeros, arg) => zeros.push(dfg.first_result(inst)),
            InstructionData::Unary {
                opcode: Opcode::Isplit,
                arg,
            } if is_zero(&zeros, arg) => zeros.extend_from_slice(dfg.inst_results(inst)),
            InstructionData::Binary {
                opcode: Opcode::Bor | Opcode::Iconcat,
                args: [a, b],
            } if is_zero(&zeros, a) && is_zero(&zeros, b) => {
                zeros.push(dfg.first_result(inst));
            }
            InstructionData::Brif { arg, .. } if is_zero(&zeros, arg) => empty.push(inst),
            InstructionData::Call { func_ref, .. }
                if releases.contains(&func_ref) && is_zero(&zeros, dfg.inst_args(inst)[0]) =>
            {
                empty.push(inst);
            }
            _ => {}
        }
    }
    state
}

/// Blocks reachable from the entry, each after all its predecessors except
/// along back edges.
fn reverse_postorder(func: &Function, cfg: &ControlFlowGraph) -> Vec<Block> {
    let Some(entry) = func.layout.entry_block() else {
        return Vec::new();
    };
    let mut seen = EntitySet::<Block>::new();
    let mut post = Vec::new();
    let mut stack = vec![(entry, cfg.succ_iter(entry))];
    seen.insert(entry);
    while let Some((block, successors)) = stack.last_mut() {
        if let Some(next) = successors.next() {
            if seen.insert(next) {
                stack.push((next, cfg.succ_iter(next)));
            }
        } else {
            post.push(*block);
            stack.pop();
        }
    }
    post.reverse();
    post
}

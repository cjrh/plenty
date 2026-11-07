//! Fuses a narrow load with the extension that widens it, so `i64(*byte)`
//! reads its byte with one `movzbq` (issue #760).
//!
//! Plenty lowers a byte read as `load.i8` and a widening cast as `uextend`
//! or `sextend`. Cranelift 0.137's x64 backend emits a zero-extending load
//! for the `load.i8` and then extends its result a second time; it fuses
//! the pair only when the IR names the extending load (`uload8` and the
//! like) directly. This pass rewrites the extension into that load and
//! leaves the original without uses; a load that cannot trap and has no
//! uses is not lowered, so the byte is read once.
//!
//! The rewritten load reads at the extension's position, so it applies only
//! when nothing between the two in the same block can write memory, and
//! only when the extension is the narrow value's sole use: another use
//! would keep the first load and make two reads.
use cranelift_codegen::entity::SecondaryMap;
use cranelift_codegen::ir::{Function, InstructionData, Opcode, Type, Value, ValueDef, types};

pub(super) fn fuse_extending_loads(func: &mut Function) {
    let mut uses: SecondaryMap<Value, u32> = SecondaryMap::new();
    let mut any = false;
    for block in func.layout.blocks() {
        for inst in func.layout.block_insts(block) {
            for value in func.dfg.inst_values(inst) {
                uses[func.dfg.resolve_aliases(value)] += 1;
            }
            any |= matches!(
                func.dfg.insts[inst],
                InstructionData::Unary {
                    opcode: Opcode::Uextend | Opcode::Sextend,
                    ..
                }
            );
        }
    }
    if !any {
        return;
    }
    let mut rewrites = Vec::new();
    for block in func.layout.blocks() {
        // Narrow loads in this block with no possible memory write since.
        let mut loads: Vec<Value> = Vec::new();
        for inst in func.layout.block_insts(block) {
            let dfg = &func.dfg;
            match dfg.insts[inst] {
                InstructionData::Unary {
                    opcode: opcode @ (Opcode::Uextend | Opcode::Sextend),
                    arg,
                } => {
                    let arg = dfg.resolve_aliases(arg);
                    let wide = dfg.value_type(dfg.first_result(inst));
                    if uses[arg] == 1
                        && loads.contains(&arg)
                        && let Some(fused) = extending_opcode(opcode, dfg.value_type(arg), wide)
                    {
                        rewrites.push((inst, arg, fused));
                    }
                }
                InstructionData::Load {
                    opcode: Opcode::Load,
                    ..
                } if dfg.insts[inst].memflags_trap_code(dfg).is_none() => {
                    let result = dfg.first_result(inst);
                    if matches!(dfg.value_type(result), types::I8 | types::I16 | types::I32) {
                        loads.push(result);
                    }
                }
                ref data => {
                    let opcode = data.opcode();
                    if opcode.can_store() || opcode.is_call() || opcode.other_side_effects() {
                        loads.clear();
                    }
                }
            }
        }
    }
    for (inst, narrow, opcode) in rewrites {
        let ValueDef::Result(load, _) = func.dfg.value_def(narrow) else {
            continue;
        };
        let InstructionData::Load {
            flags, offset, arg, ..
        } = func.dfg.insts[load]
        else {
            continue;
        };
        // The result keeps its value and wide type; a load's controlling
        // type is its result's.
        func.dfg.insts[inst] = InstructionData::Load {
            opcode,
            flags,
            offset,
            arg,
        };
    }
}

/// The load that reads `narrow` bits and extends them to `wide` as `extend`
/// does, when Cranelift defines one for that pair.
fn extending_opcode(extend: Opcode, narrow: Type, wide: Type) -> Option<Opcode> {
    let unsigned = extend == Opcode::Uextend;
    Some(match (narrow, wide) {
        (types::I8, types::I16 | types::I32 | types::I64) if unsigned => Opcode::Uload8,
        (types::I8, types::I16 | types::I32 | types::I64) => Opcode::Sload8,
        (types::I16, types::I32 | types::I64) if unsigned => Opcode::Uload16,
        (types::I16, types::I32 | types::I64) => Opcode::Sload16,
        (types::I32, types::I64) if unsigned => Opcode::Uload32,
        (types::I32, types::I64) => Opcode::Sload32,
        _ => return None,
    })
}

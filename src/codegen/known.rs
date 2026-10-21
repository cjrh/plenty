//! Compile-time evaluation of the integer CLIF a trap guard's condition is
//! built from, so a guard that cannot fire emits no branch (issue #367).
//!
//! Cranelift's optimizer folds such a condition to a constant but keeps the
//! `brif` and its trap block: `Context::optimize` removes unreachable blocks
//! before the e-graph pass that discovers the constant. So the compiler
//! decides first, in two places. `trap_if` skips a guard known false while
//! lowering, which also keeps it out of the trap-site table. A variable read
//! in a block that is not yet sealed, such as a loop header, is a block
//! parameter until sealing finds every predecessor passes the same value;
//! [`fold_constant_branches`] decides those guards once the function is built.
//! Only guards proven false disappear; a guard that is possibly or always true
//! keeps its trap and diagnostic.
use cranelift_codegen::ir::{
    DataFlowGraph, Function, InstructionData, Opcode, Value, ValueDef, condcodes::IntCC,
};

/// Replaces each `brif` whose condition is known with a `jump` to the target
/// it always takes, keeping that target's arguments. Cranelift removes the
/// blocks this leaves unreachable before it optimizes, in every mode.
pub(super) fn fold_constant_branches(func: &mut Function) {
    let mut block = func.layout.entry_block();
    while let Some(current) = block {
        if let Some(inst) = func.layout.last_inst(current)
            && let InstructionData::Brif { arg, blocks, .. } = func.dfg.insts[inst]
            && let Some(condition) = known_int(&func.dfg, arg)
        {
            func.dfg.insts[inst] = InstructionData::Jump {
                opcode: Opcode::Jump,
                destination: blocks[usize::from(condition == 0)],
            };
        }
        block = func.layout.next_block(current);
    }
}

/// Bounds the operand walk. Guard conditions sit a few instructions above
/// their constants; deeper constant expressions simply keep their guard.
const DEPTH: u32 = 8;

/// The value's bits, zero-extended from its type's width, when every input
/// it is computed from is an `iconst`.
pub(super) fn known_int(dfg: &DataFlowGraph, value: Value) -> Option<u64> {
    evaluate(dfg, value, DEPTH, &|_| None)
}

/// [`known_int`], also treating each block parameter `params` knows as that
/// constant: the value seen on one incoming edge.
pub(super) fn known_int_given(
    dfg: &DataFlowGraph,
    value: Value,
    params: &dyn Fn(Value) -> Option<u64>,
) -> Option<u64> {
    evaluate(dfg, value, DEPTH, params)
}

/// The value of an `f32const` or `f64const`, widened to `f64` (exactly).
pub(super) fn known_float(dfg: &DataFlowGraph, value: Value) -> Option<f64> {
    let ValueDef::Result(inst, _) = dfg.value_def(dfg.resolve_aliases(value)) else {
        return None;
    };
    match dfg.insts[inst] {
        InstructionData::UnaryIeee64 {
            opcode: Opcode::F64const,
            imm,
        } => Some(f64::from_bits(imm.bits())),
        InstructionData::UnaryIeee32 {
            opcode: Opcode::F32const,
            imm,
        } => Some(f64::from(f32::from_bits(imm.bits()))),
        _ => None,
    }
}

fn evaluate(
    dfg: &DataFlowGraph,
    value: Value,
    depth: u32,
    params: &dyn Fn(Value) -> Option<u64>,
) -> Option<u64> {
    let value = dfg.resolve_aliases(value);
    let bits = width(dfg, value)?;
    let (inst, index) = match dfg.value_def(value) {
        ValueDef::Result(inst, index) => (inst, index),
        ValueDef::Param(..) => return params(value).map(|x| truncate(x, bits)),
        _ => return None,
    };
    let depth = depth.checked_sub(1)?;
    let operand = |v: Value| evaluate(dfg, v, depth, params);
    let result = match dfg.insts[inst] {
        InstructionData::UnaryImm {
            opcode: Opcode::Iconst,
            imm,
        } => imm.bits() as u64,
        InstructionData::IntCompare {
            args: [a, b], cond, ..
        } => u64::from(compare(cond, operand(a)?, operand(b)?, width(dfg, a)?)),
        InstructionData::IntCompareImm { arg, cond, imm, .. } => {
            let arg_bits = width(dfg, arg)?;
            let imm = truncate(imm.bits() as u64, arg_bits);
            u64::from(compare(cond, operand(arg)?, imm, arg_bits))
        }
        InstructionData::Binary {
            opcode: Opcode::Band,
            args: [a, b],
        } => {
            // A known zero decides the result even if the other side is unknown.
            let (x, y) = (operand(a), operand(b));
            if x == Some(0) || y == Some(0) {
                0
            } else {
                x? & y?
            }
        }
        InstructionData::Binary {
            opcode,
            args: [a, b],
        } => binary(opcode, index, operand(a)?, operand(b)?, width(dfg, a)?)?,
        InstructionData::BinaryImm64 {
            opcode: Opcode::IaddImm,
            arg,
            imm,
        } => operand(arg)?.wrapping_add(imm.bits() as u64),
        InstructionData::Unary { opcode, arg } => {
            let x = operand(arg)?;
            match opcode {
                Opcode::Uextend | Opcode::Ireduce => x,
                Opcode::Sextend => signed(x, width(dfg, arg)?) as u64,
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(truncate(result, bits))
}

/// Arithmetic on known operands of width `bits`. `index` selects the wrapped
/// result (0) or the overflow flag (1) of the `*_overflow` instructions; the
/// flag is narrower than the operands, so `bits` is not the result's width.
fn binary(opcode: Opcode, index: usize, x: u64, y: u64, bits: u32) -> Option<u64> {
    let (sx, sy) = (i128::from(signed(x, bits)), i128::from(signed(y, bits)));
    let (ux, uy) = (i128::from(x), i128::from(y));
    let exact = match opcode {
        Opcode::Bor => return Some(x | y),
        Opcode::Bxor => return Some(x ^ y),
        // CLIF reduces shift counts modulo the operand width.
        Opcode::Ishl => return Some(x << (y % u64::from(bits))),
        Opcode::Ushr => return Some(x >> (y % u64::from(bits))),
        Opcode::Sshr => return Some((signed(x, bits) >> (y % u64::from(bits))) as u64),
        Opcode::Iadd | Opcode::UaddOverflow => ux + uy,
        Opcode::Isub | Opcode::UsubOverflow => ux - uy,
        Opcode::Imul | Opcode::UmulOverflow => ux * uy,
        Opcode::SaddOverflow => sx + sy,
        Opcode::SsubOverflow => sx - sy,
        Opcode::SmulOverflow => sx * sy,
        _ => return None,
    };
    let wrapped = truncate(exact as u64, bits);
    Some(match (opcode, index) {
        (_, 0) => wrapped,
        (Opcode::UaddOverflow | Opcode::UsubOverflow | Opcode::UmulOverflow, 1) => {
            u64::from(exact != i128::from(wrapped))
        }
        (Opcode::SaddOverflow | Opcode::SsubOverflow | Opcode::SmulOverflow, 1) => {
            u64::from(exact != i128::from(signed(wrapped, bits)))
        }
        _ => return None,
    })
}

fn compare(cond: IntCC, x: u64, y: u64, bits: u32) -> bool {
    let (sx, sy) = (signed(x, bits), signed(y, bits));
    match cond {
        IntCC::Equal => x == y,
        IntCC::NotEqual => x != y,
        IntCC::SignedLessThan => sx < sy,
        IntCC::SignedGreaterThanOrEqual => sx >= sy,
        IntCC::SignedGreaterThan => sx > sy,
        IntCC::SignedLessThanOrEqual => sx <= sy,
        IntCC::UnsignedLessThan => x < y,
        IntCC::UnsignedGreaterThanOrEqual => x >= y,
        IntCC::UnsignedGreaterThan => x > y,
        IntCC::UnsignedLessThanOrEqual => x <= y,
    }
}

/// Scalar integer types up to 64 bits; anything else is never known.
fn width(dfg: &DataFlowGraph, value: Value) -> Option<u32> {
    let ty = dfg.value_type(value);
    (ty.is_int() && ty.bits() <= 64).then(|| ty.bits())
}

fn truncate(x: u64, bits: u32) -> u64 {
    if bits == 64 { x } else { x & ((1 << bits) - 1) }
}

fn signed(x: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((x << shift) as i64) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{Block, InstBuilder, types};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    /// Builds one block from `body` and evaluates the value it returns.
    fn known(body: impl FnOnce(&mut FunctionBuilder, Value) -> Value) -> Option<u64> {
        let mut func = Function::new();
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut context);
        let block = builder.create_block();
        builder.append_block_param(block, types::I64);
        builder.switch_to_block(block);
        let unknown = builder.block_params(block)[0];
        let value = body(&mut builder, unknown);
        known_int(&builder.func.dfg, value)
    }

    #[test]
    fn shift_count_compare_against_literal() {
        assert_eq!(
            known(|b, _| {
                let count = b.ins().iconst(types::I64, 13);
                b.ins()
                    .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, count, 64)
            }),
            Some(0)
        );
        assert_eq!(
            known(|b, _| {
                let count = b.ins().iconst(types::I8, -1);
                b.ins()
                    .icmp_imm(IntCC::UnsignedGreaterThanOrEqual, count, 8)
            }),
            Some(1)
        );
    }

    #[test]
    fn band_with_known_false_side_is_false() {
        assert_eq!(
            known(|b, x| {
                let min = b.ins().iconst(types::I64, i64::MIN);
                let minus_one = b.ins().iconst(types::I64, -1);
                let ten = b.ins().iconst(types::I64, 10);
                let is_min = b.ins().icmp(IntCC::Equal, x, min);
                let is_minus_one = b.ins().icmp(IntCC::Equal, ten, minus_one);
                b.ins().band(is_min, is_minus_one)
            }),
            Some(0)
        );
        assert_eq!(
            known(|b, x| {
                let min = b.ins().iconst(types::I64, i64::MIN);
                let minus_one = b.ins().iconst(types::I64, -1);
                let is_min = b.ins().icmp(IntCC::Equal, x, min);
                let is_minus_one = b.ins().icmp(IntCC::Equal, minus_one, minus_one);
                b.ins().band(is_min, is_minus_one)
            }),
            None
        );
    }

    #[test]
    fn overflow_flags_at_boundaries() {
        let flag = |op: Opcode, ty, x: i64, y: i64| {
            known(|b, _| {
                let x = b.ins().iconst(ty, x);
                let y = b.ins().iconst(ty, y);
                let (_, flag) = match op {
                    Opcode::SaddOverflow => b.ins().sadd_overflow(x, y),
                    Opcode::UaddOverflow => b.ins().uadd_overflow(x, y),
                    Opcode::SsubOverflow => b.ins().ssub_overflow(x, y),
                    Opcode::UsubOverflow => b.ins().usub_overflow(x, y),
                    Opcode::SmulOverflow => b.ins().smul_overflow(x, y),
                    Opcode::UmulOverflow => b.ins().umul_overflow(x, y),
                    _ => unreachable!(),
                };
                flag
            })
        };
        use Opcode::*;
        assert_eq!(flag(SaddOverflow, types::I8, 127, 1), Some(1));
        assert_eq!(flag(SaddOverflow, types::I8, 126, 1), Some(0));
        assert_eq!(flag(SaddOverflow, types::I8, -128, -1), Some(1));
        assert_eq!(flag(UaddOverflow, types::I8, 255, 1), Some(1));
        assert_eq!(flag(UaddOverflow, types::I8, 254, 1), Some(0));
        assert_eq!(
            flag(SsubOverflow, types::I16, i64::from(i16::MIN), 1),
            Some(1)
        );
        assert_eq!(flag(UsubOverflow, types::I32, 0, 1), Some(1));
        assert_eq!(flag(UsubOverflow, types::I32, 1, 1), Some(0));
        assert_eq!(flag(SmulOverflow, types::I64, i64::MIN, -1), Some(1));
        assert_eq!(flag(SmulOverflow, types::I64, i64::MAX, 1), Some(0));
        assert_eq!(flag(UmulOverflow, types::I64, 1 << 32, 1 << 32), Some(1));
        assert_eq!(flag(UmulOverflow, types::I64, -1, 1), Some(0));
        assert_eq!(flag(UaddOverflow, types::I64, -1, 1), Some(1));
        assert_eq!(flag(SaddOverflow, types::I64, i64::MAX, 1), Some(1));
        // The flag is i8; the operands keep their own width.
        assert_eq!(flag(UaddOverflow, types::I32, 200, 100), Some(0));
        assert_eq!(flag(SaddOverflow, types::I64, 200, -100), Some(0));
    }

    #[test]
    fn constant_expression_divisor() {
        // `x // (5 + 5)`: the checked sum is known, so is its zero test.
        assert_eq!(
            known(|b, _| {
                let five = b.ins().iconst(types::I64, 5);
                let (sum, _) = b.ins().uadd_overflow(five, five);
                let zero = b.ins().iconst(types::I64, 0);
                b.ins().icmp(IntCC::Equal, sum, zero)
            }),
            Some(0)
        );
        assert_eq!(
            known(|b, _| {
                let five = b.ins().iconst(types::I64, 5);
                let (difference, _) = b.ins().usub_overflow(five, five);
                let zero = b.ins().iconst(types::I64, 0);
                b.ins().icmp(IntCC::Equal, difference, zero)
            }),
            Some(1)
        );
    }

    /// `brif (7 == 7 + offset), then(1), else(2)` with both targets taking a
    /// block argument; returns the folded terminator and the targets.
    fn fold(offset: i64) -> (InstructionData, Block, Block, Function) {
        let mut func = Function::new();
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut func, &mut context);
        let (entry, then, otherwise) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        for block in [then, otherwise] {
            builder.append_block_param(block, types::I64);
        }
        builder.switch_to_block(entry);
        let seven = builder.ins().iconst(types::I64, 7);
        let other = builder.ins().iadd_imm(seven, offset);
        let condition = builder.ins().icmp(IntCC::Equal, seven, other);
        let one = builder.ins().iconst(types::I64, 1);
        let two = builder.ins().iconst(types::I64, 2);
        builder
            .ins()
            .brif(condition, then, &[one.into()], otherwise, &[two.into()]);
        for block in [then, otherwise] {
            builder.switch_to_block(block);
            let value = builder.block_params(block)[0];
            builder.ins().return_(&[value]);
        }
        builder.seal_all_blocks();
        builder.finalize();
        fold_constant_branches(&mut func);
        let last = func.layout.last_inst(entry).unwrap();
        (func.dfg.insts[last], then, otherwise, func)
    }

    #[test]
    fn known_branches_become_jumps_with_their_arguments() {
        for (offset, taken, value) in [(0, 0, 1), (1, 1, 2)] {
            let (data, then, otherwise, func) = fold(offset);
            let InstructionData::Jump { destination, .. } = data else {
                panic!("{data:?}");
            };
            assert_eq!(
                destination.block(&func.dfg.value_lists),
                [then, otherwise][taken]
            );
            let arguments: Vec<_> = destination
                .args(&func.dfg.value_lists)
                .map(|argument| argument.as_value().unwrap())
                .collect();
            assert_eq!(known_int(&func.dfg, arguments[0]), Some(value));
        }
    }

    #[test]
    fn shifts_reduce_their_count() {
        let shift = |op: Opcode, ty, x: i64, y: i64| {
            known(|b, _| {
                let x = b.ins().iconst(ty, x);
                let y = b.ins().iconst(ty, y);
                match op {
                    Opcode::Ishl => b.ins().ishl(x, y),
                    Opcode::Ushr => b.ins().ushr(x, y),
                    Opcode::Sshr => b.ins().sshr(x, y),
                    _ => unreachable!(),
                }
            })
        };
        assert_eq!(shift(Opcode::Ishl, types::I64, 1, 4), Some(16));
        assert_eq!(shift(Opcode::Ishl, types::I8, 1, 7), Some(0x80));
        assert_eq!(shift(Opcode::Ishl, types::I8, 3, 7), Some(0x80));
        assert_eq!(shift(Opcode::Ishl, types::I8, 1, 9), Some(2));
        assert_eq!(shift(Opcode::Ushr, types::I8, -128, 7), Some(1));
        assert_eq!(shift(Opcode::Sshr, types::I8, -128, 7), Some(0xff));
        assert_eq!(shift(Opcode::Sshr, types::I32, -8, 1), Some(0xffff_fffc));
    }

    #[test]
    fn unknown_inputs_stay_unknown() {
        assert_eq!(
            known(|b, x| b.ins().icmp_imm(IntCC::UnsignedGreaterThanOrEqual, x, 64)),
            None
        );
        assert_eq!(
            known(|b, x| {
                let zero = b.ins().iconst(types::I64, 0);
                b.ins().icmp(IntCC::Equal, x, zero)
            }),
            None
        );
    }

    #[test]
    fn extensions_and_narrowing() {
        // A signed narrowing cast check: sextend(ireduce(v)) != v.
        let narrowing = |v: i64| {
            known(|b, _| {
                let value = b.ins().iconst(types::I64, v);
                let low = b.ins().ireduce(types::I8, value);
                let restored = b.ins().sextend(types::I64, low);
                b.ins().icmp(IntCC::NotEqual, restored, value)
            })
        };
        assert_eq!(narrowing(-128), Some(0));
        assert_eq!(narrowing(127), Some(0));
        assert_eq!(narrowing(128), Some(1));
        assert_eq!(narrowing(-129), Some(1));
        assert_eq!(
            known(|b, _| {
                let value = b.ins().iconst(types::I8, -1);
                b.ins().uextend(types::I64, value)
            }),
            Some(255)
        );
    }
}

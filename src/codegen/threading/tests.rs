use super::*;
use cranelift_codegen::ir::{AbiParam, Signature, condcodes::IntCC, types};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// Blocks of `a or b` feeding `if`: `entry` branches on `a`; `left` passes
/// `True` and `right` passes `b` to `join`, which branches to `yes` or `no`.
struct Or {
    func: Function,
    left: Block,
    right: Block,
    join: Block,
    yes: Block,
    no: Block,
}

/// Builds `if (x > 3) or (x > 4)`; `body` builds the join block's condition
/// from its `bool` parameter.
fn or(body: impl FnOnce(&mut FunctionBuilder, Value) -> Value) -> Or {
    let mut func = Function::new();
    func.signature = Signature::new(CallConv::SystemV);
    func.signature.params.push(AbiParam::new(types::I64));
    func.signature.returns.push(AbiParam::new(types::I64));
    let mut context = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut context);
    let [entry, left, right, join, yes, no] = [(); 6].map(|_| b.create_block());
    b.append_block_params_for_function_params(entry);
    let flag = b.append_block_param(join, types::I8);
    b.switch_to_block(entry);
    let x = b.block_params(entry)[0];
    let a = b.ins().icmp_imm(IntCC::SignedGreaterThan, x, 3);
    b.ins().brif(a, left, &[], right, &[]);
    b.switch_to_block(left);
    let one = b.ins().iconst(types::I8, 1);
    b.ins().jump(join, &[one.into()]);
    b.switch_to_block(right);
    let c = b.ins().icmp_imm(IntCC::SignedGreaterThan, x, 4);
    b.ins().jump(join, &[c.into()]);
    b.switch_to_block(join);
    let condition = body(&mut b, flag);
    b.ins().brif(condition, yes, &[], no, &[]);
    for (block, value) in [(yes, 1), (no, 2)] {
        b.switch_to_block(block);
        let value = b.ins().iconst(types::I64, value);
        b.ins().return_(&[value]);
    }
    b.seal_all_blocks();
    b.finalize();
    Or {
        func,
        left,
        right,
        join,
        yes,
        no,
    }
}

fn verify(func: &Function) {
    let flags = settings::Flags::new(settings::builder());
    cranelift_codegen::verify_function(func, &flags).unwrap_or_else(|e| panic!("{e}\n{func}"));
}

fn terminator(func: &Function, block: Block) -> InstructionData {
    func.dfg.insts[func.layout.last_inst(block).unwrap()]
}

fn jump_target(func: &Function, block: Block) -> Option<Block> {
    match terminator(func, block) {
        InstructionData::Jump { destination, .. } => Some(destination.block(&func.dfg.value_lists)),
        _ => None,
    }
}

fn branch_targets(func: &Function, block: Block) -> Option<(Value, Block, Block)> {
    match terminator(func, block) {
        InstructionData::Brif { arg, blocks, .. } => Some((
            arg,
            blocks[0].block(&func.dfg.value_lists),
            blocks[1].block(&func.dfg.value_lists),
        )),
        _ => None,
    }
}

#[test]
fn constant_edge_jumps_to_its_target_and_unknown_edge_branches() {
    let mut or = or(|_, flag| flag);
    thread_branches(&mut or.func);
    verify(&or.func);
    assert_eq!(jump_target(&or.func, or.left), Some(or.yes));
    let (condition, yes, no) = branch_targets(&or.func, or.right).expect("brif in right");
    assert_eq!((yes, no), (or.yes, or.no));
    // The right operand's own comparison, not the join parameter.
    assert_eq!(
        or.func
            .layout
            .inst_block(match or.func.dfg.value_def(condition) {
                ValueDef::Result(inst, _) => inst,
                other => panic!("{other:?}"),
            }),
        Some(or.right)
    );
}

#[test]
fn negated_parameter_swaps_the_targets() {
    let mut or = or(|b, flag| {
        let one = b.ins().iconst(types::I8, 1);
        b.ins().bxor(flag, one)
    });
    thread_branches(&mut or.func);
    verify(&or.func);
    assert_eq!(jump_target(&or.func, or.left), Some(or.no));
    // `not b` on the unknown edge branches on `b` with the targets swapped.
    let (_, yes, no) = branch_targets(&or.func, or.right).expect("brif in right");
    assert_eq!((yes, no), (or.no, or.yes));
}

#[test]
fn parameter_used_after_the_join_is_not_skipped() {
    let mut or = or(|_, flag| flag);
    // Use the join parameter in `yes`, as reading `x = a or b` later would.
    let flag = or.func.dfg.block_params(or.join)[0];
    let ret = or.func.layout.last_inst(or.yes).unwrap();
    FuncCursor::new(&mut or.func)
        .at_inst(ret)
        .ins()
        .uextend(types::I64, flag);
    let before = or.func.display().to_string();
    thread_branches(&mut or.func);
    verify(&or.func);
    assert_eq!(or.func.display().to_string(), before);
}

#[test]
fn trapping_instruction_keeps_its_block() {
    let mut or = or(|b, flag| {
        let wide = b.ins().uextend(types::I64, flag);
        let quotient = b.ins().udiv(wide, wide);
        b.ins().ireduce(types::I8, quotient)
    });
    let before = or.func.display().to_string();
    thread_branches(&mut or.func);
    verify(&or.func);
    assert_eq!(or.func.display().to_string(), before);
}

#[test]
fn constant_computed_in_a_skipped_block_is_rebuilt_on_the_edge() {
    // `join(p): brif p == 1, out(p + 1), no`, with `p` known on the `left` edge.
    let mut or = or(|_, flag| flag);
    let func = &mut or.func;
    let out = func.dfg.make_block();
    func.layout.append_block(out);
    let value = func.dfg.append_block_param(out, types::I8);
    let join_branch = func.layout.last_inst(or.join).unwrap();
    let flag = func.dfg.block_params(or.join)[0];
    let mut cursor = FuncCursor::new(func).at_inst(join_branch);
    let sum = cursor.ins().iadd_imm(flag, 1);
    let one = cursor.ins().icmp_imm(IntCC::Equal, flag, 1);
    let blocks = [
        cursor.func.dfg.block_call(out, &[BlockArg::Value(sum)]),
        cursor.func.dfg.block_call(or.no, &[]),
    ];
    cursor.func.dfg.insts[join_branch] = InstructionData::Brif {
        opcode: Opcode::Brif,
        arg: one,
        blocks,
    };
    let mut cursor = FuncCursor::new(func).at_bottom(out);
    let wide = cursor.ins().uextend(types::I64, value);
    cursor.ins().return_(&[wide]);
    thread_branches(func);
    verify(func);
    let InstructionData::Jump { destination, .. } = terminator(func, or.left) else {
        panic!("{func}");
    };
    assert_eq!(destination.block(&func.dfg.value_lists), out);
    let argument = destination.args(&func.dfg.value_lists).next().unwrap();
    let argument = argument.as_value().unwrap();
    assert_eq!(known_int(&func.dfg, argument), Some(2));
    assert_eq!(defining_block(func, argument), Some(or.left));
    // The condition is not a parameter, so the unknown edge keeps the join.
    assert_eq!(jump_target(func, or.right), Some(or.join));
}

#[test]
fn constant_infinite_loop_terminates() {
    // `looping(p): brif p, looping(0), looping(1)`, entered with `looping(1)`.
    let mut func = Function::new();
    let mut context = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut context);
    let [entry, looping] = [(); 2].map(|_| b.create_block());
    let p = b.append_block_param(looping, types::I8);
    b.switch_to_block(entry);
    let one = b.ins().iconst(types::I8, 1);
    b.ins().jump(looping, &[one.into()]);
    b.switch_to_block(looping);
    let zero = b.ins().iconst(types::I8, 0);
    let one = b.ins().iconst(types::I8, 1);
    b.ins()
        .brif(p, looping, &[zero.into()], looping, &[one.into()]);
    b.seal_all_blocks();
    b.finalize();
    thread_branches(&mut func);
    verify(&func);
}

//! Isolated backend qualification: does not enable a Plenty production target.
use cranelift_codegen::{
    Context,
    ir::*,
    isa::CallConv,
    settings::{self, Configurable},
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

fn define(
    module: &mut ObjectModule,
    id: cranelift_module::FuncId,
    sig: Signature,
    body: impl FnOnce(&mut ObjectModule, &mut FunctionBuilder<'_>),
) {
    let mut ctx = Context::new();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    b.seal_block(block);
    body(module, &mut b);
    b.finalize(module.isa().frontend_config());
    println!("{}", ctx.func.display());
    module
        .define_function(id, &mut ctx)
        .expect("lower ARM function");
}

fn main() {
    let output = std::env::args_os().nth(1).expect("output ELF object path");
    let mut flags = settings::builder();
    flags.set("is_pic", "true").unwrap();
    flags.set("enable_probestack", "true").unwrap();
    flags.set("probestack_strategy", "inline").unwrap();
    let isa = cranelift_codegen::isa::lookup("aarch64-unknown-linux-gnu".parse().unwrap())
        .unwrap()
        .finish(settings::Flags::new(flags))
        .unwrap();
    println!(
        "triple={} flags={} isa_flags={}",
        isa.triple(),
        isa.flags(),
        isa.isa_flags()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut m = ObjectModule::new(
        ObjectBuilder::new(isa, "arm-probe", cranelift_module::default_libcall_names()).unwrap(),
    );
    let mut tail = Signature::new(CallConv::Tail);
    tail.params = vec![AbiParam::new(types::I64); 20];
    tail.returns.push(AbiParam::new(types::I64));
    let sum = m
        .declare_function("tail_sum", Linkage::Local, &tail)
        .unwrap();
    let direct = m
        .declare_function("tail_direct", Linkage::Local, &tail)
        .unwrap();
    let indirect = m
        .declare_function("tail_indirect", Linkage::Local, &tail)
        .unwrap();
    define(&mut m, sum, tail.clone(), |_, b| {
        let args = b.block_params(b.current_block().unwrap()).to_vec();
        let value = args.into_iter().reduce(|a, c| b.ins().iadd(a, c)).unwrap();
        b.ins().return_(&[value]);
    });
    define(&mut m, direct, tail.clone(), |m, b| {
        let callee = m.declare_func_in_func(sum, b.func);
        let args = b.block_params(b.current_block().unwrap()).to_vec();
        b.ins().return_call(callee, &args);
    });
    define(&mut m, indirect, tail, |m, b| {
        let callee = m.declare_func_in_func(direct, b.func);
        let addr = b.ins().func_addr(types::I64, callee);
        let signature = b.import_signature(b.func.signature.clone());
        let args = b.block_params(b.current_block().unwrap()).to_vec();
        b.ins().return_call_indirect(signature, addr, &args);
    });
    let mut c128 = Signature::new(CallConv::SystemV);
    c128.params = vec![AbiParam::new(types::I128); 2];
    c128.returns.push(AbiParam::new(types::I128));
    let add = m
        .declare_function("c_add128", Linkage::Import, &c128)
        .unwrap();
    let roundtrip = m
        .declare_function("probe_u128", Linkage::Export, &c128)
        .unwrap();
    define(&mut m, roundtrip, c128, |m, b| {
        let callee = m.declare_func_in_func(add, b.func);
        let args = b.block_params(b.current_block().unwrap()).to_vec();
        let call = b.ins().call(callee, &args);
        let results = b.inst_results(call).to_vec();
        b.ins().return_(&results);
    });
    let mut touch_sig = Signature::new(CallConv::SystemV);
    touch_sig.params.push(AbiParam::new(types::I64));
    let touch = m
        .declare_function("c_touch", Linkage::Import, &touch_sig)
        .unwrap();
    let data = m
        .declare_data("probe_data", Linkage::Local, false, false)
        .unwrap();
    let mut desc = DataDescription::new();
    desc.define(17u64.to_le_bytes().to_vec().into_boxed_slice());
    m.define_data(data, &desc).unwrap();
    let mut entry_sig = Signature::new(CallConv::SystemV);
    entry_sig.returns.push(AbiParam::new(types::I64));
    let entry = m
        .declare_function("probe_entry", Linkage::Export, &entry_sig)
        .unwrap();
    define(&mut m, entry, entry_sig.clone(), |m, b| {
        let slot =
            b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 131072, 4));
        let pointer = b.ins().stack_addr(types::I64, slot, 0);
        let c_touch = m.declare_func_in_func(touch, b.func);
        b.ins().call(c_touch, &[pointer]);
        let first = b
            .ins()
            .load(types::I64, MemFlagsData::trusted(), pointer, 0);
        let callee = m.declare_func_in_func(indirect, b.func);
        let args: Vec<_> = (1..=20).map(|n| b.ins().iconst(types::I64, n)).collect();
        let call = b.ins().call(callee, &args);
        let sum = b.inst_results(call)[0];
        let global = m.declare_data_in_func(data, b.func);
        let addr = b.ins().symbol_value(types::I64, global);
        let constant = b.ins().load(types::I64, MemFlagsData::trusted(), addr, 0);
        let result = b.ins().iadd(sum, first);
        let result = b.ins().iadd(result, constant);
        b.ins().return_(&[result]);
    });
    let trap = m
        .declare_function("probe_trap", Linkage::Export, &entry_sig)
        .unwrap();
    define(&mut m, trap, entry_sig, |_, b| {
        b.ins().trap(TrapCode::unwrap_user(1));
    });
    std::fs::write(output, m.finish().emit().unwrap()).unwrap();
}

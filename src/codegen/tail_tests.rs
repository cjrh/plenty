//! Test representation lowering independently of source lifetime eligibility.
use super::*;

#[test]
fn approved_reference_and_result_addresses_are_forwarded_without_snapshots() {
    let range = Ty::Range(Arc::new(Ty::I64));
    let sig = Arc::new(FnSig {
        inputs: vec![("external".into(), Ty::Ref(Arc::new(range.clone()), false))],
        outputs: vec![range],
    });
    let builder = cranelift_object::ObjectBuilder::new(
        host_isa().unwrap(),
        "tail-test",
        cranelift_module::default_libcall_names(),
    )
    .unwrap();
    let mut module = ObjectModule::new(builder);
    let runtime = declare_runtime(&mut module, false).unwrap();
    let id = module
        .declare_function("callee", Linkage::Local, &user_fn_signature(&sig))
        .unwrap();
    let mut functions = HashMap::default();
    functions.insert(
        "callee".into(),
        UserFn {
            inline: None,
            standard_library: false,
            drop_callback: None,
            generator: None,
            resume: None,
            frame_mask: None,
            id,
            sig: sig.clone(),
            locals: Arc::from([]),
            source: None,
            preserves_caller: false,
        },
    );
    let empty = module
        .declare_data("empty", Linkage::Local, false, false)
        .unwrap();
    let mut symbols = symbols::Symbols::new(Arc::new(symbols::Declared::snapshot(
        &module,
        host_isa().unwrap(),
    )));
    let mut function =
        Function::with_name_signature(UserFuncName::user(0, 0), user_fn_signature(&sig));
    let mut context = cranelift_frontend::FunctionBuilderContext::new();
    let mut bcx = FunctionBuilder::new(&mut function, &mut context);
    let entry = bcx.create_block();
    bcx.append_block_params_for_function_params(entry);
    bcx.switch_to_block(entry);
    bcx.seal_block(entry);
    let incoming = bcx.block_params(entry).to_vec();
    let strings = HashMap::default();
    let mut lower = Lowerer {
        bcx: &mut bcx,
        module: &mut symbols,
        runtime: &runtime,
        user_fns: &functions,
        str_data: &strings,
        eof_empty_str: empty,
        locals: &[],
        stack: vec![(incoming[0], sig.inputs[0].1.clone())],
        terminated: false,
        loop_targets: Vec::new(),
        generator: None,
        local_frame: None,
        return_storage: Some(incoming[1]),
        return_types: &sig.outputs,
        collection_scratch: None,
        fresh_load: None,
        scalar_captures: Default::default(),
        slot_copies: Vec::new(),
        empty_slot: None,
        counters: Default::default(),
        position: Default::default(),
    };
    lower.lower_tail_call("callee").unwrap();
    assert!(lower.terminated);
    let last = lower.bcx.func.layout.last_inst(entry).unwrap();
    assert_eq!(
        lower.bcx.func.dfg.insts[last].opcode(),
        cranelift_codegen::ir::Opcode::ReturnCall
    );
    assert_eq!(lower.bcx.func.dfg.inst_args(last), incoming);
    assert!(lower.bcx.func.sized_stack_slots.is_empty());
    bcx.finalize();
}

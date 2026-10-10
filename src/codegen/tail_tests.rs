//! Test representation lowering independently of source lifetime eligibility.
use super::*;

#[test]
fn approved_reference_and_result_addresses_are_forwarded_without_snapshots() {
    let range = Ty::Range(Rc::new(Ty::I64));
    let sig = Rc::new(FnSig {
        inputs: vec![("external".into(), Ty::Ref(Rc::new(range.clone()), false))],
        outputs: vec![range],
    });
    let builder = cranelift_object::ObjectBuilder::new(
        host_isa().unwrap(),
        "tail-test",
        cranelift_module::default_libcall_names(),
    )
    .unwrap();
    let mut module = ObjectModule::new(builder);
    let runtime = declare_runtime(&mut module).unwrap();
    let id = module
        .declare_function("callee", Linkage::Local, &user_fn_signature(&module, &sig))
        .unwrap();
    let mut functions = HashMap::new();
    functions.insert(
        "callee".into(),
        UserFn {
            drop_callback: None,
            generator: None,
            resume: None,
            id,
            sig: sig.clone(),
            body: Rc::from([]),
            locals: Rc::from([]),
        },
    );
    let empty = module
        .declare_data("empty", Linkage::Local, false, false)
        .unwrap();
    let mut function =
        Function::with_name_signature(UserFuncName::user(0, 0), user_fn_signature(&module, &sig));
    let mut context = cranelift_frontend::FunctionBuilderContext::new();
    let mut bcx = FunctionBuilder::new(&mut function, &mut context);
    let entry = bcx.create_block();
    bcx.append_block_params_for_function_params(entry);
    bcx.switch_to_block(entry);
    bcx.seal_block(entry);
    let incoming = bcx.block_params(entry).to_vec();
    let strings = HashMap::new();
    let mut lower = Lowerer {
        bcx: &mut bcx,
        module: &mut module,
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

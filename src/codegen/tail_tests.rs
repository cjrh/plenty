//! Test lowering independently of what source programs reach: tail-call
//! representations, and a body inlined although an expanded loop is too large
//! to be inlined today.
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
            inline_body: None,
            column_predicate: None,
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
            reads_no_caller: false,
            names_caller_in_sites: false,
        },
    );
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
    let strings = StrData {
        literals: HashMap::default(),
        byte_lens: HashMap::default(),
        tables: HashMap::default(),
    };
    let mut lower = Lowerer {
        bcx: &mut bcx,
        module: &mut symbols,
        runtime: &runtime,
        user_fns: &functions,
        str_data: &strings,
        locals: &[],
        stack: vec![(incoming[0], sig.inputs[0].1.clone())],
        terminated: false,
        assumed_index: false,
        loop_targets: Vec::new(),
        checked_targets: Vec::new(),
        generator: None,
        local_frame: None,
        return_storage: Some(incoming[1]),
        return_types: &sig.outputs,
        collection_scratch: None,
        fresh_load: None,
        tags: Default::default(),
        scalar_captures: Default::default(),
        portable_simd: Default::default(),
        pairs: Default::default(),
        slot_copies: Vec::new(),
        empty_slot: None,
        exits: None,
        counters: Default::default(),
        position: Default::default(),
        trap_blocks: HashMap::default(),
        moves: Default::default(),
        joins: Default::default(),
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
    lower.seal_trap_blocks();
    lower.seal_joins();
    bcx.finalize(crate::codegen::FRONTEND);
}

/// An inlined body chooses its own rows. No source program reaches this yet:
/// only an expanded loop packs cheap statements, and the smallest function
/// with one exceeds `inline_calls`' size bound.
#[test]
#[cfg(target_arch = "x86_64")]
fn an_inlined_body_keeps_rows() {
    let sig = Arc::new(FnSig {
        inputs: vec![
            ("n".into(), Ty::I64),
            ("p".into(), Ty::F64),
            ("q".into(), Ty::F64),
        ],
        outputs: vec![Ty::F64],
    });
    let isa = || optimized_isa(crate::CpuLevel::Baseline, "backtracking", "speed", false).unwrap();
    let builder = cranelift_object::ObjectBuilder::new(
        isa(),
        "inline-test",
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
            inline_body: None,
            column_predicate: None,
            standard_library: false,
            drop_callback: None,
            generator: None,
            resume: None,
            frame_mask: None,
            id,
            sig: sig.clone(),
            locals: Arc::from([Ty::F64, Ty::F64, Ty::F64, Ty::F64]),
            source: None,
            preserves_caller: false,
            reads_no_caller: false,
            names_caller_in_sites: false,
        },
    );
    let mut symbols = symbols::Symbols::new(Arc::new(symbols::Declared::snapshot(&module, isa())));
    assert!(symbols.packs_statements());
    let mut function =
        Function::with_name_signature(UserFuncName::user(0, 0), user_fn_signature(&sig));
    let mut context = cranelift_frontend::FunctionBuilderContext::new();
    let mut bcx = FunctionBuilder::new(&mut function, &mut context);
    let entry = bcx.create_block();
    bcx.append_block_params_for_function_params(entry);
    bcx.switch_to_block(entry);
    bcx.seal_block(entry);
    let incoming = bcx.block_params(entry).to_vec();
    let strings = StrData {
        literals: HashMap::default(),
        byte_lens: HashMap::default(),
        tables: HashMap::default(),
    };
    let mut lower = Lowerer {
        bcx: &mut bcx,
        module: &mut symbols,
        runtime: &runtime,
        user_fns: &functions,
        str_data: &strings,
        locals: &[],
        stack: Vec::new(),
        terminated: false,
        assumed_index: false,
        loop_targets: Vec::new(),
        checked_targets: Vec::new(),
        generator: None,
        local_frame: None,
        return_storage: None,
        return_types: &sig.outputs,
        collection_scratch: None,
        fresh_load: None,
        tags: Default::default(),
        scalar_captures: Default::default(),
        portable_simd: Default::default(),
        pairs: Default::default(),
        slot_copies: Vec::new(),
        empty_slot: None,
        exits: None,
        counters: Default::default(),
        position: Default::default(),
        trap_blocks: HashMap::default(),
        moves: Default::default(),
        joins: Default::default(),
    };
    // x = p; y = q; c = 1.5; d = -2.25
    // while n > 0: x = x + c; y = y + d
    // return x - y
    let float = |value: f64| Op::PushFloat {
        bits: value.to_bits(),
        ty: Ty::F64,
    };
    let body = [
        Op::LoadLocal(1),
        Op::StoreLocal(3),
        Op::LoadLocal(2),
        Op::StoreLocal(4),
        float(1.5),
        Op::StoreLocal(5),
        float(-2.25),
        Op::StoreLocal(6),
        Op::Loop {
            condition: Arc::from([
                Op::LoadLocal(0),
                Op::PushInt(crate::value::Value::I64(0)),
                Op::Gt,
            ]),
            body: Arc::from([
                Op::LoadLocal(3),
                Op::LoadLocal(5),
                Op::Add,
                Op::StoreLocal(3),
                Op::LoadLocal(4),
                Op::LoadLocal(6),
                Op::Add,
                Op::StoreLocal(4),
            ]),
        },
        Op::LoadLocal(3),
        Op::LoadLocal(4),
        Op::Sub,
        Op::Return,
    ];
    lower
        .lower_inlined_call("callee", &body, incoming[..3].to_vec(), false, &[])
        .unwrap();
    let (value, _) = lower.stack.pop().unwrap();
    lower.bcx.ins().return_(&[value]);
    lower.seal_trap_blocks();
    lower.seal_joins();
    bcx.finalize(crate::codegen::FRONTEND);
    let text = function.display().to_string();
    // `x` and `y` are one row and `c` and `d` another: the loop adds rows,
    // and the difference reads both lanes of the first.
    assert_eq!(text.matches(" = fadd ").count(), 1, "{text}");
    assert!(text.contains("f64x2"), "{text}");
    assert_eq!(text.matches(" = extractlane").count(), 2, "{text}");
}

//! Native adapters for formatting methods that `print` and `str.repr` run.
//!
//! The runtime renders a value through its type metadata. A type whose
//! `display` or `debug` method a formatting operation uses carries the
//! address of one of these adapters, which receives a reference to the value
//! and a cell for the method's `Result[str, FormatError]`.
use super::*;
use crate::collection::{CollectionOp, FormatHook};

/// Declare one adapter per hook function, in name order, and record the
/// first for each type and method in `hooks`, where the metadata finds it.
/// Hooks for the same type and method call the same method, so any will do.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
pub(super) fn declare_adapters(
    ops: &[Op],
    workers: usize,
    module: &mut ObjectModule,
    hooks: &mut HashMap<(Ty, bool), FuncId>,
) -> Result<Vec<(FormatHook, FuncId)>> {
    let found: std::collections::BTreeMap<_, _> =
        crate::threading::find(ops, workers, |op| match op {
            Op::Collection(CollectionOp::Formatted(formatted)) => Some(
                formatted
                    .hooks
                    .iter()
                    .map(|hook| (hook.adapter(), hook.clone()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .into_iter()
        .flatten()
        .collect();
    let mut declared = Vec::new();
    for (name, hook) in found {
        let id =
            module.declare_function(&name, Linkage::Local, &signature(module.make_signature()))?;
        hooks.entry((hook.ty.clone(), hook.display)).or_insert(id);
        declared.push((hook, id));
    }
    Ok(declared)
}

/// Value reference, result cell -> nothing.
fn signature(mut signature: Signature) -> Signature {
    signature
        .params
        .extend([AbiParam::new(PTR_TY), AbiParam::new(PTR_TY)]);
    signature
}

pub(super) fn build_adapter(
    hook: &FormatHook,
    id: FuncId,
    functions: &HashMap<String, UserFn>,
    str_data: &StrData,
    runtime: &Runtime,
    out: &mut Worker,
) -> Result<()> {
    let mut module = out.symbols();
    let signature = signature(module.make_signature());
    let mut func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
    let mut bcx = FunctionBuilder::new(&mut func, &mut out.builder);
    let entry = bcx.create_block();
    bcx.append_block_params_for_function_params(entry);
    bcx.switch_to_block(entry);
    bcx.seal_block(entry);
    let params = bcx.block_params(entry).to_vec();
    let mut lower = Lowerer {
        bcx: &mut bcx,
        module: &mut module,
        runtime,
        user_fns: functions,
        str_data,
        locals: &[],
        stack: vec![],
        terminated: false,
        assumed_index: false,
        loop_targets: vec![],
        checked_targets: vec![],
        generator: None,
        local_frame: None,
        return_storage: None,
        return_types: &[],
        collection_scratch: None,
        fresh_load: None,
        tags: Default::default(),
        scalar_captures: Default::default(),
        portable_simd: Default::default(),
        slot_copies: Vec::new(),
        empty_slot: None,
        exits: None,
        counters: Default::default(),
        position: Default::default(),
        trap_blocks: HashMap::default(),
        moves: Default::default(),
    };
    let reference = lower.bcx.ins().uextend(types::I128, params[0]);
    lower
        .stack
        .push((reference, Ty::Ref(Arc::new(hook.ty.clone()), false)));
    // The hook builds its own formatter with the f-string limits
    // (`frontend::fstrings::method_function`).
    lower.lower_call(&hook.function)?;
    let (result, ty) = lower
        .stack
        .pop()
        .ok_or("formatting hook returned nothing")?;
    let result = lower.pack(result, &ty);
    lower.bcx.ins().store(
        cranelift_codegen::ir::MemFlagsData::trusted(),
        result,
        params[1],
        0,
    );
    lower.bcx.ins().return_(&[]);
    lower.seal_trap_blocks();
    bcx.finalize(crate::codegen::FRONTEND);
    out.define(id, func, module);
    Ok(())
}

//! Native comparators for runtime-driven key sorts.
//!
//! `plenty_list_sort_by` moves rows inside the runtime and calls one of these
//! adapters per comparison with the key callback reference and two row values.
//! The adapter computes both keys through the specialized `key_of` helper and
//! compares them in the total sort order.
use super::*;
use crate::collection::{CollectionOp, KeySort};

/// Declare one adapter per key sort, in name order.
pub(super) fn declare_adapters(
    ops: &[Op],
    workers: usize,
    module: &mut ObjectModule,
    adapters: &mut HashMap<String, FuncId>,
) -> Result<Vec<(Arc<KeySort>, FuncId)>> {
    // An adapter's name identifies its key helper, so any one use of it will do.
    let sorts: std::collections::BTreeMap<_, _> =
        crate::threading::find(ops, workers, |op| match op {
            Op::Collection(CollectionOp::ListSortBy(sort)) => {
                Some((sort.adapter.clone(), sort.clone()))
            }
            _ => None,
        })
        .into_iter()
        .collect();
    let mut declared = Vec::new();
    for (name, sort) in sorts {
        let id =
            module.declare_function(&name, Linkage::Local, &signature(module.make_signature()))?;
        adapters.insert(name, id);
        declared.push((sort, id));
    }
    Ok(declared)
}

/// Callback reference, left row slot, right row slot -> order as -1, 0, or 1.
fn signature(mut signature: Signature) -> Signature {
    signature.params.extend([
        AbiParam::new(PTR_TY),
        AbiParam::new(PTR_TY),
        AbiParam::new(PTR_TY),
    ]);
    signature.returns.push(AbiParam::new(types::I64));
    signature
}

pub(super) fn build_adapter(
    sort: &KeySort,
    id: FuncId,
    functions: &HashMap<String, UserFn>,
    str_data: &HashMap<StrId, DataId>,
    eof_empty_str: DataId,
    runtime: &Runtime,
    out: &mut Worker,
) -> Result<()> {
    let Ty::List(element) = &sort.list else {
        return Err("key sorting requires a list".into());
    };
    let item = Ty::Ref(element.clone(), false);
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
        eof_empty_str,
        locals: &[],
        stack: vec![],
        terminated: false,
        loop_targets: vec![],
        generator: None,
        local_frame: None,
        return_storage: None,
        return_types: &[],
        collection_scratch: None,
        fresh_load: None,
        scalar_captures: Default::default(),
        slot_copies: Vec::new(),
        empty_slot: None,
        counters: Default::default(),
        position: Default::default(),
        trap_blocks: HashMap::default(),
    };
    // References are slot addresses: the runtime passes each row's value word
    // through a slot it owns for the duration of the call.
    let callback = lower.bcx.ins().uextend(types::I128, params[0]);
    for row in &params[1..] {
        let row = lower.bcx.ins().uextend(types::I128, *row);
        lower.stack.push((callback, sort.callback.clone()));
        lower.stack.push((row, item.clone()));
        lower.lower_call(&sort.key_of)?;
    }
    lower.lower_collection(&CollectionOp::OrderCompare(sort.key.clone()))?;
    let (order, _) = lower.pop_typed(Ty::I64)?;
    lower.bcx.ins().return_(&[order]);
    lower.seal_trap_blocks();
    bcx.finalize();
    out.define(id, func, module);
    Ok(())
}

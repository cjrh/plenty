//! Native comparators for runtime-driven key sorts.
//!
//! `plenty_list_sort_by` moves rows inside the runtime and calls one of these
//! adapters per comparison with the key callback reference and two row values.
//! The adapter computes both keys through the specialized `key_of` helper and
//! compares them in the total sort order.
use super::*;
use crate::collection::{CollectionOp, KeySort};
use cranelift_codegen::ir::MemFlagsData;

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
    str_data: &StrData,
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
        locals: &[],
        stack: vec![],
        terminated: false,
        loop_targets: vec![],
        checked_targets: vec![],
        generator: None,
        local_frame: None,
        return_storage: None,
        return_types: &[],
        collection_scratch: None,
        fresh_load: None,
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
    // References are slot addresses: the runtime passes each row's value word
    // through a slot it owns for the duration of the call.
    let checked = sort.error.is_some();
    let callback = if checked {
        let flags = MemFlagsData::trusted();
        let previous = lower.bcx.ins().load(types::I128, flags, params[0], 16);
        let tag = lower.sum_tag(previous);
        let failed = lower.bcx.ins().icmp_imm_s(IntCC::NotEqual, tag, 0);
        let done = lower.bcx.create_block();
        let begin = lower.bcx.create_block();
        lower.bcx.ins().brif(failed, done, &[], begin, &[]);
        lower.bcx.switch_to_block(done);
        lower.bcx.seal_block(done);
        let equal = lower.bcx.ins().iconst(types::I64, 0);
        lower.bcx.ins().return_(&[equal]);
        lower.bcx.switch_to_block(begin);
        lower.bcx.seal_block(begin);
        lower.bcx.ins().load(types::I128, flags, params[0], 0)
    } else {
        lower.bcx.ins().uextend(types::I128, params[0])
    };
    // A class reference names a slot whose record follows it. The runtime's
    // slot holds only the row's record address, so name the 16 bytes before
    // that record instead.
    let record = element.flat_field();
    for row in &params[1..] {
        let row = if record {
            let address = lower.bcx.ins().load(
                PTR_TY,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                *row,
                0,
            );
            lower.bcx.ins().iadd_imm_s(address, -16)
        } else {
            *row
        };
        let row = lower.bcx.ins().uextend(types::I128, row);
        lower.stack.push((callback, sort.callback.clone()));
        lower.stack.push((row, item.clone()));
        lower.lower_call(&sort.key_of)?;
        if checked {
            lower.checked_sort_key(params[0], &sort.key)?;
        }
    }
    lower.lower_collection(&CollectionOp::OrderCompare(sort.key.clone()))?;
    let (order, _) = lower.pop_typed(Ty::I64)?;
    lower.bcx.ins().return_(&[order]);
    lower.seal_trap_blocks();
    bcx.finalize(crate::codegen::FRONTEND);
    out.define(id, func, module);
    Ok(())
}

impl Lowerer<'_, '_> {
    /// The runtime keeps moving rows after a failure, but the adapter stops
    /// evaluating user keys. The caller reads this cell once sorting returns.
    fn checked_sort_key(&mut self, context: cranelift_codegen::ir::Value, key: &Ty) -> Result<()> {
        let (value, result) = self.stack.pop().ok_or("missing checked sort key")?;
        let Ty::Enum(_) = result else {
            return Err("checked key must return Result".into());
        };
        let tag = self.sum_tag(value);
        let payload = self.sum_payload(value);
        let failed = self.bcx.ins().icmp_imm_s(IntCC::NotEqual, tag, 0);
        let failure = self.bcx.create_block();
        let success = self.bcx.create_block();
        self.bcx.ins().brif(failed, failure, &[], success, &[]);
        self.bcx.switch_to_block(failure);
        self.bcx.seal_block(failure);
        let pending = self.stack.clone();
        self.release_stack();
        let error = self.wrap_sum(payload, 1);
        self.bcx
            .ins()
            .store(MemFlagsData::trusted(), error, context, 16);
        let equal = self.bcx.ins().iconst(types::I64, 0);
        self.bcx.ins().return_(&[equal]);
        self.stack = pending;
        self.bcx.switch_to_block(success);
        self.bcx.seal_block(success);
        let key_value = self.unpack(payload, key);
        self.stack.push((key_value, key.clone()));
        Ok(())
    }
}

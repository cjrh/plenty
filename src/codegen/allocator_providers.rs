//! Private scalar C adapters for certified allocator providers.
use super::*;
use crate::threading::{AllocatorProvider, ThreadOp};

pub(super) fn name(provider: &AllocatorProvider, allocate: bool) -> String {
    format!(
        "__plenty_provider_{}",
        if allocate {
            &provider.allocate
        } else {
            &provider.release
        }
    )
}
fn signature(mut sig: Signature, allocate: bool) -> Signature {
    sig.params.extend(std::iter::repeat_n(
        AbiParam::new(PTR_TY),
        if allocate { 4 } else { 2 },
    ));
    if allocate {
        sig.returns.push(AbiParam::new(types::I64));
    }
    sig
}
/// Search all emitted functions: selected owners can reach other callees long
/// after their factory returns. Only needed when a compiled provider exists.
pub(super) fn selected_owners(ops: &[Op]) -> ProviderEffects {
    use crate::collection_opcodes::{ALLOCATOR_BYTES_SLICE, ALLOCATOR_CLIST_SLICE};
    use crate::foreign::{Intrinsic, Target};
    let mut effects = ProviderEffects::default();
    crate::threading::walk(ops, &mut |op| {
        if let Op::ForeignCall { declaration, .. } = op
            && let Target::Intrinsic(intrinsic) = &declaration.target
            && let Intrinsic::Allocator { opcode, .. } = **intrinsic
        {
            effects.bytes |= opcode == ALLOCATOR_BYTES_SLICE;
            effects.clists |= opcode == ALLOCATOR_CLIST_SLICE;
        }
    });
    effects
}

pub(super) fn declare_adapters(
    ops: &[Op],
    workers: usize,
    module: &mut ObjectModule,
) -> Result<Vec<(Arc<AllocatorProvider>, bool, FuncId)>> {
    let providers: std::collections::BTreeMap<_, _> =
        crate::threading::find(ops, workers, |op| match op {
            Op::Thread(ThreadOp::AllocatorProvider(p)) => Some((p.allocate.clone(), p.clone())),
            _ => None,
        })
        .into_iter()
        .collect();
    let mut adapters = Vec::new();
    for provider in providers.into_values() {
        for allocate in [true, false] {
            let id = module.declare_function(
                &name(&provider, allocate),
                Linkage::Local,
                &signature(module.make_signature(), allocate),
            )?;
            adapters.push((provider.clone(), allocate, id));
        }
    }
    Ok(adapters)
}
pub(super) fn build_adapter(
    provider: &AllocatorProvider,
    allocate: bool,
    id: FuncId,
    functions: &HashMap<String, UserFn>,
    str_data: &StrData,
    runtime: &Runtime,
    out: &mut Worker,
) -> Result<()> {
    let mut module = out.symbols();
    let sig = signature(module.make_signature(), allocate);
    let mut func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), sig);
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

    let slot = lower
        .bcx
        .ins()
        .iadd_imm_s(params[0], enums::box_content_offset(&provider.state));
    let reference = lower.bcx.ins().uextend(types::I128, slot);
    lower
        .stack
        .push((reference, Ty::Ref(Arc::new(provider.state.clone()), true)));
    let storage = Ty::Array(Arc::new(Ty::U64), 3);
    if allocate {
        lower.stack.push((params[1], Ty::I64));
        lower.stack.push((params[2], Ty::I64));
        lower.lower_call(&provider.allocate)?;
        let (result, ty) = lower
            .stack
            .pop()
            .ok_or("missing provider allocation result")?;
        let Ty::Enum(tuple) = ty else {
            unreachable!("private provider tuple")
        };
        lower.stack.push((result, Ty::Enum(tuple.clone())));
        lower.lower_split(&tuple, 0)?;
        let (words, _) = lower.stack.pop().ok_or("missing provider block words")?;
        let (status, _) = lower.stack.pop().ok_or("missing provider status")?;
        let words = lower.raw_word(words);
        lower.copy_bytes(params[3], words, storage.inline_bytes() as u64);
        lower.bcx.ins().return_(&[status]);
    } else {
        // One native Block becomes one owned ByteBlock in the private wrapper.
        let words = lower.bcx.ins().uextend(types::I128, params[1]);
        let words = lower.snapshot_inline(words, &storage);
        lower.stack.push((words, storage));
        lower.lower_call(&provider.release)?;
        lower.bcx.ins().return_(&[]);
    }
    lower.seal_trap_blocks();
    lower.seal_joins();
    bcx.finalize(crate::codegen::FRONTEND);
    out.define(id, func, module);
    Ok(())
}

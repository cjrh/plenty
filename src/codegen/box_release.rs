//! Compiled release of a box's content, and in-place box construction
//! (issue #740).
//!
//! The runtime destroys a box by walking its content's descriptor with
//! `release_next`, returning to the destruction queue at each heap child and
//! resuming the walk afterwards. For content whose release runs no code, such
//! as a tree node holding `Option[Box[Node]]` fields, the order in which
//! children are released and freed is unobservable. Its box descriptor then
//! carries one of these adapters, which reads each owned child at its
//! compile-time offset and hands it to `plenty_release`. During destruction
//! that call only queues the child, so the adapter never recurses and deep
//! values still drop on a small stack. See "Box destruction" in
//! book/src/design/toolchain/02-architecture/memory.md.
use super::*;
use crate::boxed::BoxOp;
use cranelift_codegen::ir::{MemFlagsData, Value as NativeValue};

/// Whether `ty` releases with one `plenty_release` of its value word, which
/// during destruction only queues the object.
fn heap_handle(ty: &Ty) -> bool {
    ty.managed()
        && !ty.wide()
        && !matches!(ty, Ty::Generator(_) | Ty::Closure(_))
        && !ty.has_inline_storage()
}

/// The tag and field of the one variant of the sum `ty` that owns anything,
/// when that variant holds a single [`heap_handle`] in the payload word, as in
/// `Option[Box[T]]` or `Option[str]`.
pub(super) fn heap_payload(ty: &Ty) -> Option<(usize, Ty)> {
    let Ty::Enum(t) = ty else { return None };
    if ty.has_inline_storage() {
        return None;
    }
    let definition = t.try_get()?;
    let mut owning = definition
        .variants
        .iter()
        .enumerate()
        .filter(|(_, v)| v.fields.iter().any(Ty::managed));
    let (tag, variant) = owning.next()?;
    if owning.next().is_some() {
        return None;
    }
    match variant.fields.as_slice() {
        [field] if heap_handle(field) => Some((tag, field.clone())),
        _ => None,
    }
}

/// A box content slot's owned children, as byte offsets from the record and
/// their types, when the adapter can release them; see the module comment.
fn children(content: &Ty) -> Option<Children> {
    if !content.managed() || content.release_runs_code() {
        return None;
    }
    if heap_handle(content) || heap_payload(content).is_some() {
        return Some(Children::Slot(content.clone()));
    }
    let Ty::Class(t) = content else { return None };
    let class = t.try_get()?;
    if class.destructor.is_some() || class.newtype_field().is_some() || !content.flat_record() {
        return None;
    }
    let mut fields = Vec::new();
    for (index, (_, field)) in class.fields.iter().enumerate() {
        if !field.managed() {
            continue;
        }
        if field.flat_field() || !(heap_handle(field) || heap_payload(field).is_some()) {
            return None;
        }
        fields.push((class.field_offset(index) as i32, field.clone()));
    }
    Some(Children::Record(fields))
}

enum Children {
    /// The slot holds the child itself.
    Slot(Ty),
    /// The slot's value word addresses a class record with these fields.
    Record(Vec<(i32, Ty)>),
}

/// Declare one adapter per box content type that [`children`] accepts and a
/// `Box(...)` constructs, in name order so the object does not depend on the
/// worker count, and record each in `releasers`, where the metadata finds it.
#[expect(
    clippy::mutable_key_type,
    reason = "nominal identities exclude definition cells"
)]
pub(super) fn declare_adapters(
    ops: &[Op],
    workers: usize,
    module: &mut ObjectModule,
    releasers: &mut HashMap<Ty, FuncId>,
) -> Result<Vec<(Ty, FuncId)>> {
    let found: std::collections::BTreeMap<_, _> =
        crate::threading::find(ops, workers, |op| match op {
            Op::Box(BoxOp::New(t) | BoxOp::Share(t)) if children(t).is_some() => {
                Some((t.identity_name(), t.clone()))
            }
            _ => None,
        })
        .into_iter()
        .collect();
    let mut declared = Vec::new();
    for (name, ty) in found {
        let id = module.declare_function(
            &format!("__plenty_box_release[{name}]"),
            Linkage::Local,
            &signature(module.make_signature()),
        )?;
        releasers.insert(ty.clone(), id);
        declared.push((ty, id));
    }
    Ok(declared)
}

/// Content slot -> nothing, as the runtime's `Type::drop` hook.
fn signature(mut signature: Signature) -> Signature {
    signature.call_conv = crate::target::native_call_conv();
    signature.params.push(AbiParam::new(PTR_TY));
    signature
}

pub(super) fn build_adapter(
    content: &Ty,
    id: FuncId,
    functions: &HashMap<String, UserFn>,
    str_data: &StrData,
    runtime: &Runtime,
    out: &mut Worker,
) -> Result<()> {
    let children = children(content).ok_or("box release adapter for unsupported content")?;
    let mut module = out.symbols();
    let signature = signature(module.make_signature());
    let mut func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), signature);
    let mut bcx = FunctionBuilder::new(&mut func, &mut out.builder);
    let entry = bcx.create_block();
    bcx.append_block_params_for_function_params(entry);
    bcx.switch_to_block(entry);
    bcx.seal_block(entry);
    let slot = bcx.block_params(entry)[0];
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
    let flags = MemFlagsData::trusted();
    match children {
        Children::Slot(ty) => {
            let packed = lower.bcx.ins().load(types::I128, flags, slot, 0);
            let value = lower.unpack(packed, &ty);
            lower.release(value, &ty);
        }
        Children::Record(fields) => {
            // A box whose content was moved out keeps a zero value word.
            let base = lower.bcx.ins().load(PTR_TY, flags, slot, 0);
            let release = lower.bcx.create_block();
            let done = lower.bcx.create_block();
            lower.bcx.ins().brif(base, release, &[], done, &[]);
            lower.bcx.switch_to_block(release);
            lower.bcx.seal_block(release);
            for (offset, ty) in fields {
                let packed = lower.bcx.ins().load(types::I128, flags, base, offset);
                let value = lower.unpack(packed, &ty);
                lower.release(value, &ty);
            }
            lower.bcx.ins().jump(done, &[]);
            lower.bcx.switch_to_block(done);
            lower.bcx.seal_block(done);
        }
    }
    lower.bcx.ins().return_(&[]);
    lower.seal_trap_blocks();
    bcx.finalize(crate::codegen::FRONTEND);
    out.define(id, func, module);
    Ok(())
}

impl Lowerer<'_, '_> {
    /// Release a sum [`heap_payload`] accepts: test the tag, then release the
    /// payload word as its field, without the runtime's descriptor walk.
    pub(super) fn release_heap_payload(&mut self, value: NativeValue, ty: &Ty) -> bool {
        let (Some((tag, field)), Ty::Enum(t)) = (heap_payload(ty), ty) else {
            return false;
        };
        let mask = (t.get().variants.len().next_power_of_two() - 1).max(1);
        let (_, tags) = enums::split(self.bcx, value);
        let tags = self.bcx.ins().band_imm_s(tags, mask as i64);
        let owning = self.bcx.ins().icmp_imm_s(IntCC::Equal, tags, tag as i64);
        let release = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.ins().brif(owning, release, &[], done, &[]);
        self.bcx.switch_to_block(release);
        self.bcx.seal_block(release);
        let payload = self.unpack(value, &field);
        self.release(payload, &field);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        true
    }

    /// `Box(value)` for content with inline storage: the runtime allocates
    /// the box, or releases `value` when it cannot, and compiled code copies
    /// the content into its slot, as a local's store does, rather than the
    /// runtime walking the content's descriptor to copy it.
    pub(super) fn box_in_place(&mut self, value: NativeValue, content: &Ty) -> Result<NativeValue> {
        let packed = self.pack(value, content);
        let boxed = Ty::Box(std::sync::Arc::new(content.clone()));
        let result = self.collection_call(137, &[packed], Some(&boxed))?;
        let (word, tags) = enums::split(self.bcx, result);
        let failed = self.bcx.ins().band_imm_s(tags, 1);
        let store = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.ins().brif(failed, done, &[], store, &[]);
        self.bcx.switch_to_block(store);
        self.bcx.seal_block(store);
        let slot = self.bcx.ins().iadd_imm_s(word, 32);
        self.store_slot(slot, value, content);
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        Ok(result)
    }
}

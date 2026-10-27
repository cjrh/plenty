//! Inline value storage. Runtime operands carry scalar-sized addresses, while
//! escaping ranges and generator frames live in storage belonging to their owner.
use super::*;
use cranelift_codegen::ir::{MemFlagsData, Value as NativeValue};

impl Lowerer<'_, '_> {
    /// Copy `bytes` (a multiple of 8) from `source` to `destination`, which
    /// may overlap. Cranelift inlines copies of up to 32 bytes and calls
    /// `memmove` above that; a nested record is usually larger, and the call
    /// cost several times the copy (issue #550). Up to [`INLINE_COPY_LIMIT`]
    /// bytes are therefore moved in 16-byte vector registers, all loaded
    /// before any is stored so an overlapping copy stays correct.
    pub(super) fn copy_bytes(&mut self, destination: NativeValue, source: NativeValue, bytes: u64) {
        let config = self.module.target_config();
        if bytes <= 32 || bytes > INLINE_COPY_LIMIT {
            self.bcx.emit_small_memory_copy(
                config,
                destination,
                source,
                bytes,
                8,
                8,
                false,
                MemFlagsData::trusted(),
            );
            return;
        }
        let flags = MemFlagsData::new().with_notrap();
        let mut loaded = Vec::new();
        let mut offset = 0;
        while offset < bytes {
            let ty = if bytes - offset >= 16 {
                types::I64X2
            } else {
                types::I64
            };
            let value = self.bcx.ins().load(ty, flags, source, offset as i32);
            loaded.push((value, offset));
            offset += ty.bytes() as u64;
        }
        for (value, offset) in loaded {
            self.bcx
                .ins()
                .store(flags, value, destination, offset as i32);
        }
    }

    /// Zero `bytes` (a multiple of 8) at `destination`, in vector stores up to
    /// [`INLINE_COPY_LIMIT`] bytes for the reason given at [`Self::copy_bytes`].
    pub(super) fn zero_bytes(&mut self, destination: NativeValue, bytes: u64) {
        let config = self.module.target_config();
        if bytes <= 32 || bytes > INLINE_COPY_LIMIT {
            self.bcx
                .emit_small_memset(config, destination, 0, bytes, 8, MemFlagsData::trusted());
            return;
        }
        let flags = MemFlagsData::new().with_notrap();
        let zero = self.bcx.ins().iconst(types::I64, 0);
        let wide = self.bcx.ins().splat(types::I64X2, zero);
        let mut offset = 0;
        while offset < bytes {
            let value = if bytes - offset >= 16 { wide } else { zero };
            self.bcx
                .ins()
                .store(flags, value, destination, offset as i32);
            offset += if bytes - offset >= 16 { 16 } else { 8 };
        }
    }

    pub(super) fn inline_storage(&mut self, bytes: usize) -> NativeValue {
        let slot = self
            .bcx
            .create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                bytes as u32,
                4,
            ));
        self.bcx.ins().stack_addr(PTR_TY, slot, 0)
    }

    pub(super) fn copy_inline_to(
        &mut self,
        value: NativeValue,
        ty: &Ty,
        destination: NativeValue,
    ) -> NativeValue {
        if !ty.has_inline_storage() {
            return value;
        }
        // A newtype's 128 bits are its field's: copy that.
        if let Some(field) = ty.newtype_field() {
            let value = self.unpack(value, &field);
            let copied = self.copy_inline_to(value, &field, destination);
            return self.pack(copied, &field);
        }
        if ty.flat_record() || matches!(ty, Ty::Class(_)) && record_fixes(ty).is_some() {
            return self.copy_record(value, ty, destination);
        }
        if !ty.wide()
            && let Some(bytes) = plain_bytes(ty)
        {
            let copied = self.copy_words(value, destination, bytes);
            if matches!(ty, Ty::Closure(_)) {
                self.copy_capture_values(value, copied);
            }
            return copied;
        }
        // A null value word owns no inline payload, and the runtime returns
        // such a value unchanged. Testing it here keeps calls out of the
        // common payload-free cases, such as `Ok(())` returned as a
        // `Result[(), E]` whose error holds a record (issue #121).
        let packed = self.pack(value, ty);
        let word = self.raw_word(packed);
        let copy = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I128);
        self.bcx.ins().brif(word, copy, &[], done, &[packed.into()]);
        self.bcx.switch_to_block(copy);
        self.bcx.seal_block(copy);
        let copied = if let Ty::Enum(t) = ty
            && let Some(variants) = plain_variants(t)
        {
            self.copy_plain_variant(packed, destination, t, &variants)
        } else {
            self.collection_call(117, &[packed, destination], Some(ty))
                .expect("inline payload metadata")
        };
        self.bcx.ins().jump(done, &[copied.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        let result = self.bcx.block_params(done)[0];
        self.unpack(result, ty)
    }

    /// Copy the payload of a sum value whose payload word is not null, given
    /// the bytes each variant with inline storage keeps (see
    /// [`plain_variants`]). Other variants own nothing inline and keep their
    /// value. The tags stay as they are, as in the runtime's `copy_payload`.
    fn copy_plain_variant(
        &mut self,
        packed: NativeValue,
        destination: NativeValue,
        ty: &crate::nominal::Nominal<crate::sum::EnumType>,
        variants: &[(usize, u64)],
    ) -> NativeValue {
        let (word, tags) = self.bcx.ins().isplit(packed);
        let tag = self.bcx.ins().band_imm_s(tags, (1i64 << ty.tag_bits()) - 1);
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I128);
        for &(variant, bytes) in variants {
            let copy = self.bcx.create_block();
            let next = self.bcx.create_block();
            let matches = self.bcx.ins().icmp_imm_s(IntCC::Equal, tag, variant as i64);
            self.bcx.ins().brif(matches, copy, &[], next, &[]);
            self.bcx.switch_to_block(copy);
            self.bcx.seal_block(copy);
            self.copy_bytes(destination, word, bytes);
            self.fix_variant(&ty.get().variants[variant].fields, destination);
            let moved = self.bcx.ins().iconcat(destination, tags);
            self.bcx.ins().jump(done, &[moved.into()]);
            self.bcx.switch_to_block(next);
            self.bcx.seal_block(next);
        }
        self.bcx.ins().jump(done, &[packed.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
    }

    /// Copy a record whose nested payloads sit at offsets known here (see
    /// [`record_fixes`]): its bytes, then the address of each nested payload.
    /// A null value word owns nothing and is not copied.
    fn copy_record(
        &mut self,
        value: NativeValue,
        ty: &Ty,
        destination: NativeValue,
    ) -> NativeValue {
        let source = self.raw_word(value);
        let copy = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, types::I128);
        self.bcx
            .ins()
            .brif(source, copy, &[], done, &[value.into()]);
        self.bcx.switch_to_block(copy);
        self.bcx.seal_block(copy);
        self.copy_bytes(destination, source, ty.inline_bytes() as u64);
        if let Ty::Class(_) = ty {
            self.fix_record(ty, destination);
        }
        let tags = self.high_word(value);
        let moved = self.bcx.ins().iconcat(destination, tags);
        self.bcx.ins().jump(done, &[moved.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
    }

    /// Point the nested payload addresses of a class record just copied to
    /// `base` at their new places, as the runtime's `Payload::relocate` does.
    fn fix_record(&mut self, ty: &Ty, base: NativeValue) {
        let Ty::Class(t) = ty else {
            unreachable!("record fixes apply to classes")
        };
        let mut offset = t.get().state_bytes() as i32;
        for (_, field) in &t.get().fields {
            self.fix_field(field, base, offset);
            offset += field.field_bytes() as i32;
        }
    }

    /// `fix_record` for the payload of a sum variant just copied to `base`.
    /// A single field is the payload itself; several form a record laid out
    /// as a class's.
    fn fix_variant(&mut self, fields: &[Ty], base: NativeValue) {
        match fields {
            [] => {}
            [field] => {
                let field = field.newtype_base();
                if matches!(*field, Ty::Class(_)) {
                    self.fix_record(&field, base);
                }
            }
            fields => {
                let mut offset = 0;
                for field in fields {
                    self.fix_field(field, base, offset);
                    offset += field.field_bytes() as i32;
                }
            }
        }
    }

    /// Fix the field at `base + offset` of a record just copied to `base`.
    fn fix_field(&mut self, field: &Ty, base: NativeValue, offset: i32) {
        if field.flat_field() {
            // A flat field has no address of its own; only its payloads move.
            if !field.flat_record() {
                let record = self.bcx.ins().iadd_imm_s(base, offset as i64);
                self.fix_record(&field.newtype_base(), record);
            }
        } else {
            self.fix_slot(field, base, offset);
        }
    }

    /// Move the payload address of the slot at `base + offset` to the inline
    /// storage that follows it, when the slot holds a payload. `record_fixes`
    /// accepted `field`, so its payload layout is known here.
    fn fix_slot(&mut self, field: &Ty, base: NativeValue, offset: i32) {
        if !field.has_inline_storage() {
            return;
        }
        let field = &*field.newtype_base();
        let flags = MemFlagsData::trusted();
        let word = self.bcx.ins().load(types::I64, flags, base, offset);
        let fix = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.ins().brif(word, fix, &[], done, &[]);
        self.bcx.switch_to_block(fix);
        self.bcx.seal_block(fix);
        let payload = self.bcx.ins().iadd_imm_s(base, offset as i64 + 16);
        match field {
            Ty::Enum(t) => {
                let tags = self.bcx.ins().load(types::I64, flags, base, offset + 8);
                let tag = self.bcx.ins().band_imm_s(tags, (1i64 << t.tag_bits()) - 1);
                for (variant, v) in t.get().variants.iter().enumerate() {
                    if !variant_storage(&v.fields) {
                        continue;
                    }
                    let moved = self.bcx.create_block();
                    let next = self.bcx.create_block();
                    let matches = self.bcx.ins().icmp_imm_s(IntCC::Equal, tag, variant as i64);
                    self.bcx.ins().brif(matches, moved, &[], next, &[]);
                    self.bcx.switch_to_block(moved);
                    self.bcx.seal_block(moved);
                    self.bcx.ins().store(flags, payload, base, offset);
                    self.fix_variant(&v.fields, payload);
                    self.bcx.ins().jump(done, &[]);
                    self.bcx.switch_to_block(next);
                    self.bcx.seal_block(next);
                }
            }
            _ => {
                self.bcx.ins().store(flags, payload, base, offset);
                if matches!(field, Ty::Class(_)) {
                    self.fix_record(field, payload);
                }
            }
        }
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
    }

    /// Release a class `release_fields` accepts field by field, as the
    /// runtime's record release does, without walking its descriptor: a record
    /// whose owned fields were moved out then costs a few loads (issue #544).
    pub(super) fn release_record(&mut self, value: NativeValue, ty: &Ty) {
        let Ty::Class(t) = ty else {
            unreachable!("inline release applies to classes")
        };
        let base = self.raw_word(value);
        let release = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.ins().brif(base, release, &[], done, &[]);
        self.bcx.switch_to_block(release);
        self.bcx.seal_block(release);
        let mut offset = t.get().state_bytes() as i32;
        for (_, field) in &t.get().fields {
            if field.managed() {
                let packed = if field.flat_field() {
                    let record = self.bcx.ins().iadd_imm_s(base, offset as i64);
                    self.bcx.ins().uextend(types::I128, record)
                } else {
                    self.bcx
                        .ins()
                        .load(types::I128, MemFlagsData::trusted(), base, offset)
                };
                let field_value = self.unpack(packed, field);
                if field.wide() || matches!(field, Ty::Generator(_) | Ty::Closure(_)) {
                    // These test for an empty slot themselves.
                    self.release(field_value, field);
                } else {
                    // A field moved out of leaves a zero word behind.
                    let word = self.raw_word(field_value);
                    let owned = self.bcx.create_block();
                    let next = self.bcx.create_block();
                    self.bcx.ins().brif(word, owned, &[], next, &[]);
                    self.bcx.switch_to_block(owned);
                    self.bcx.seal_block(owned);
                    self.release(field_value, field);
                    self.bcx.ins().jump(next, &[]);
                    self.bcx.switch_to_block(next);
                    self.bcx.seal_block(next);
                }
            }
            offset += field.field_bytes() as i32;
        }
        self.bcx.ins().jump(done, &[]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
    }

    /// Copy a payload of `bytes` plain bytes with no internal addresses to
    /// relocate, such as a range's words: copying a stored range for a loop or
    /// an argument then needs no runtime call (issue #371). A null value word
    /// owns nothing and is not copied.
    fn copy_words(
        &mut self,
        value: NativeValue,
        destination: NativeValue,
        bytes: u64,
    ) -> NativeValue {
        let copy = self.bcx.create_block();
        let done = self.bcx.create_block();
        self.bcx.append_block_param(done, PTR_TY);
        self.bcx.ins().brif(value, copy, &[], done, &[value.into()]);
        self.bcx.switch_to_block(copy);
        self.bcx.seal_block(copy);
        self.copy_bytes(destination, value, bytes);
        self.bcx.ins().jump(done, &[destination.into()]);
        self.bcx.switch_to_block(done);
        self.bcx.seal_block(done);
        self.bcx.block_params(done)[0]
    }

    /// Copy the pending in-place moves aside after all: the op being lowered
    /// is not the consumer [`Moves`] expected.
    pub(super) fn settle_moves(&mut self) {
        for moved in std::mem::take(&mut self.moves.pending) {
            if let Some(index) = self.stack.iter().rposition(|(value, _)| *value == moved) {
                let ty = self.stack[index].1.clone();
                self.stack[index].0 = self.snapshot_inline(moved, &ty);
            }
        }
    }

    pub(super) fn snapshot_inline(&mut self, value: NativeValue, ty: &Ty) -> NativeValue {
        if self.portable_simd.fresh(value) {
            return value;
        }
        if !ty.has_inline_storage() {
            return value;
        }
        let destination = self.inline_storage(ty.inline_bytes());
        self.copy_inline_to(value, ty, destination)
    }

    pub(super) fn store_slot(&mut self, slot: NativeValue, value: NativeValue, ty: &Ty) {
        let value = if ty.has_inline_storage() {
            let destination = self.bcx.ins().iadd_imm_s(slot, 16);
            self.copy_inline_to(value, ty, destination)
        } else {
            value
        };
        let packed = self.pack(value, ty);
        self.bcx
            .ins()
            .store(MemFlagsData::trusted(), packed, slot, 0);
    }

    pub(super) fn local_offset(&self, index: usize) -> i64 {
        self.locals[..index]
            .iter()
            .map(|(_, t)| t.slot_bytes() as i64)
            .sum()
    }

    pub(super) fn return_values(&mut self, values: Vec<StackEntry>) {
        let returned = self.stage_results(values);
        self.release_locals();
        self.bcx.ins().return_(&returned);
    }

    pub(super) fn stage_results(&mut self, values: Vec<StackEntry>) -> Vec<NativeValue> {
        let mut offset = 0;
        let mut returned = Vec::new();
        for (value, ty) in values {
            let value = if ty.has_inline_storage() {
                let base = self
                    .return_storage
                    .expect("caller-owned inline return storage");
                let destination = self.bcx.ins().iadd_imm_s(base, offset);
                offset += ty.inline_bytes() as i64;
                self.copy_inline_to(value, &ty, destination)
            } else {
                value
            };
            returned.push(value);
        }
        returned
    }
}

/// The largest copy or clear [`Lowerer::copy_bytes`] emits inline: twelve
/// vector registers, within x86-64's sixteen so the copy needs no spills.
/// Raising it to 256 bytes sped up no measured workload and grew the `.text`
/// of an actor example by a further 2.5%.
const INLINE_COPY_LIMIT: u64 = 192;

/// The bytes of a payload that moves without relocation: no field keeps inline
/// storage, so no internal address points into it. `None` when copying needs
/// the runtime's descriptor walk.
fn plain_bytes(ty: &Ty) -> Option<u64> {
    match &*ty.newtype_base() {
        Ty::Range(_) => Some(32),
        // An environment holds its descriptor and capture slots.
        Ty::Closure(t) if t.captures.iter().all(|(_, t)| !t.has_inline_storage()) => {
            Some(t.bytes() as u64)
        }
        base @ Ty::Class(_) if base.flat_record() => Some(base.inline_bytes() as u64),
        _ => None,
    }
}

/// Copying more nested payloads than this calls the runtime's descriptor walk
/// rather than growing the code at every copy site.
const RECORD_FIX_LIMIT: usize = 8;

/// How many nested payload addresses a copy of the class `ty` must fix, when
/// all of them sit at offsets known at compile time: classes, ranges, closures
/// whose captures keep no inline storage, and sums of those. `None` when the
/// copy needs the runtime, such as for a generator frame. A class around a
/// class around a `File` then moves with a byte copy and one address fix
/// (issue #544).
fn record_fixes(ty: &Ty) -> Option<usize> {
    let Ty::Class(t) = ty else { return None };
    // A newtype has its field's layout, not a record's.
    if t.get().newtype_field().is_some() {
        return None;
    }
    let fixes = t
        .get()
        .fields
        .iter()
        .try_fold(0, |n, (_, field)| Some(n + field_fixes(field)?))?;
    (fixes <= RECORD_FIX_LIMIT).then_some(fixes)
}

/// The address fixes for a field of a class or multi-field variant record.
/// A flat field adds only the fixes inside its record.
fn field_fixes(field: &Ty) -> Option<usize> {
    if field.flat_field() {
        record_fixes(&field.newtype_base())
    } else {
        slot_fixes(field)
    }
}

/// The address fixes for a slot holding `ty`, including those inside its payload.
fn slot_fixes(ty: &Ty) -> Option<usize> {
    if !ty.has_inline_storage() {
        return Some(0);
    }
    match &*ty.newtype_base() {
        Ty::Range(_) => Some(1),
        Ty::Closure(_) => plain_bytes(ty).map(|_| 1),
        base @ Ty::Class(_) => Some(1 + record_fixes(base)?),
        Ty::Enum(t) => t
            .get()
            .variants
            .iter()
            .try_fold(1, |n, v| Some(n + variant_fixes(&v.fields)?)),
        _ => None,
    }
}

/// The address fixes inside one variant's payload once it has moved.
fn variant_fixes(fields: &[Ty]) -> Option<usize> {
    match fields {
        [] => Some(0),
        [field] if !field.has_inline_storage() => Some(0),
        [field] => match &*field.newtype_base() {
            // A nested sum's own tags are packed into its owner's: leave those
            // to the runtime.
            Ty::Enum(_) => None,
            base @ Ty::Class(_) => record_fixes(base),
            base => slot_fixes(base).map(|n| n - 1),
        },
        fields => fields.iter().try_fold(0, |n, f| Some(n + field_fixes(f)?)),
    }
}

/// How many owned fields an inline release of the class `ty` visits, when it
/// has no destructor and that count is small; `None` leaves the release to
/// the runtime's descriptor walk. Fields that are such classes count their
/// own fields.
pub(super) fn release_fields(ty: &Ty) -> Option<usize> {
    let Ty::Class(t) = ty else { return None };
    let class = t.try_get()?;
    // A newtype releases as its field; a destructor needs the runtime.
    if class.destructor.is_some() || class.newtype_field().is_some() {
        return None;
    }
    let fields = class
        .fields
        .iter()
        .filter(|(_, f)| f.managed())
        .try_fold(0, |n, (_, f)| Some(n + release_fields(f).unwrap_or(1)))?;
    (fields <= RECORD_FIX_LIMIT).then_some(fields)
}

/// Whether a variant with these fields keeps a payload in inline storage.
fn variant_storage(fields: &[Ty]) -> bool {
    match fields {
        [] => false,
        [field] => field.has_inline_storage(),
        _ => true,
    }
}

/// For a sum, each variant with inline storage and the bytes its payload keeps,
/// when every such payload copies without the runtime: a single field with
/// [`plain_bytes`] or a class [`record_fixes`] accepts, or several fields
/// whose nested payloads [`field_fixes`] accepts. A `Result[Bytes, E]` then
/// copies without a runtime call (issue #384), as does a `Result` of a class
/// holding a class (issue #544).
fn plain_variants(ty: &crate::nominal::Nominal<crate::sum::EnumType>) -> Option<Vec<(usize, u64)>> {
    let mut variants = Vec::new();
    for (tag, variant) in ty.get().variants.iter().enumerate() {
        match variant.fields.as_slice() {
            [] => {}
            [field] if !field.has_inline_storage() => {}
            [field] => match &*field.newtype_base() {
                base @ Ty::Class(_) => {
                    record_fixes(base)?;
                    variants.push((tag, base.inline_bytes() as u64));
                }
                base => variants.push((tag, plain_bytes(base)?)),
            },
            fields => {
                let fixes = fields
                    .iter()
                    .try_fold(0, |n, f| Some(n + field_fixes(f)?))?;
                if fixes > RECORD_FIX_LIMIT {
                    return None;
                }
                variants.push((tag, Lowerer::record_offset(fields.iter()) as u64));
            }
        }
    }
    Some(variants)
}

/// Moves out of a local whose bytes the consuming operation reads in place.
///
/// Moving a value with inline storage out of a local copies its bytes aside,
/// since the local could be written again while the value is still in use.
/// When the move is an argument a call copies before returning, or a result
/// the return copies to its caller, and only operations that push values come
/// between, nothing can write the local first, so that copy is redundant
/// (issue #550).
#[derive(Default)]
pub(super) struct Moves {
    /// Such `Op::MoveLocal`s, identified by address: the body outlives its
    /// lowering.
    candidates: crate::hash::HashSet<*const Op>,
    /// The values of such moves, until their consumer is lowered. A call
    /// that might keep an argument's address copies them there instead, and
    /// [`Lowerer::settle_moves`] does for any other operation.
    pub(super) pending: Vec<NativeValue>,
}

impl Moves {
    pub(super) fn find(body: &[Op]) -> Self {
        let mut candidates = crate::hash::HashSet::default();
        let mut sequences = vec![body];
        while let Some(ops) = sequences.pop() {
            for (index, op) in ops.iter().enumerate() {
                match op {
                    Op::MoveLocal(_) => {
                        let consumer = ops[index + 1..].iter().find(|op| !Self::passes(op));
                        if matches!(consumer, Some(Op::Call(_) | Op::Return)) {
                            candidates.insert(op as *const Op);
                        }
                    }
                    Op::Match(arms) => sequences.extend(arms.iter().map(|arm| arm.body.as_ref())),
                    Op::Loop { condition, body } => {
                        sequences.extend([condition.as_ref(), body.as_ref()]);
                    }
                    Op::If {
                        then,
                        elifs,
                        otherwise,
                    } => {
                        sequences.extend([then.as_ref(), otherwise.as_ref()]);
                        for arm in elifs.iter() {
                            sequences.extend([arm.condition.as_ref(), arm.body.as_ref()]);
                        }
                    }
                    _ => {}
                }
            }
        }
        Self {
            candidates,
            pending: Vec::new(),
        }
    }

    /// Whether `op` may come between a move and its consumer: it only pushes
    /// a value or marks a position, so it writes no local.
    pub(super) fn passes(op: &Op) -> bool {
        matches!(
            op,
            Op::Site(_)
                | Op::CallerSite(_)
                | Op::Access(..)
                | Op::MoveLocal(_)
                | Op::LoadLocal(_)
                | Op::PushInt(_)
                | Op::PushFloat { .. }
                | Op::PushUnit
                | Op::PushStr(_)
                | Op::PushBool(_)
        )
    }

    /// Whether the move `op` of a value of type `ty` skips its copy.
    pub(super) fn in_place(&self, op: &Op, ty: &Ty) -> bool {
        ty.has_inline_storage() && self.candidates.contains(&(op as *const Op))
    }
}

//! Rust-owned metadata and buffers behind a small, layout-stable native prefix.
use crate::collection_opcodes::*;
use crate::entries::{Entries, Entry};
use crate::generators::{Generator, plenty_generator_resume};
use crate::memory::{self, AllocError, Header, plenty_release, plenty_retain};
use crate::ranges::{self, Range};
use crate::strings::{self, Text};
use std::cmp::Ordering;
use std::io::Write;

mod byte_scan;
mod byte_storage;
mod bytes;
mod clist;
mod compress;
mod crypto;
mod crypto_advanced;
mod crypto_keys;
mod entry_error;
pub(crate) mod executor_map;
pub(crate) mod executor_reduce;
mod filesystem;
#[cfg(test)]
mod hash_tests;
mod hashing;
pub(crate) mod net;
#[cfg(test)]
mod operand_tests;
mod process;
mod readiness;
#[cfg(test)]
mod record_tests;
#[cfg(test)]
mod removal_tests;
#[cfg(plenty_runtime_sqlite)]
mod sqlite;
mod tls;
mod watch;

#[repr(C)]
pub(crate) struct Variant {
    pub(crate) name: &'static str,
    pub(crate) fields: &'static [&'static Type],
}
#[repr(C)]
pub(crate) struct Type {
    pub(crate) kind: u8,
    pub(crate) affine: bool,
    pub(crate) reflexive: bool,
    pub(crate) inline_range: bool,
    /// Extra owner-local payload bytes, excluding the ordinary 16-byte slot.
    pub(crate) inline_bytes: u32,
    pub(crate) key: Option<&'static Type>,
    pub(crate) value: Option<&'static Type>,
    pub(crate) name: &'static str,
    pub(crate) variants: &'static [Variant],
    /// A class's `__del__` adapter, which receives the instance's slot.
    pub(crate) drop: Option<unsafe extern "C" fn(*mut u128)>,
    /// Set for a one-field class without `__del__`, whose values are its
    /// field's: the rest of this descriptor is the field's layout, and the
    /// wrapper names the field and links its own descriptor for formatting
    /// and the columnar-list schema. See book/src/proposals/flat-class-layout.md.
    pub(crate) wrapper: Option<&'static Variant>,
    /// Layout facts the compiler records for a class with a record, so
    /// release and relocation can skip their field walks (issue #550).
    pub(crate) plan: Option<&'static Plan>,
}
/// What releasing and moving a class record involves, precomputed from its
/// field types. Mirrors `codegen::metadata::plan`.
#[repr(C)]
pub(crate) struct Plan {
    /// Word offsets, from the record's first byte, of the strings it owns,
    /// including those of flat class fields.
    pub(crate) strings: &'static [u32],
    /// Releasing the record only frees `strings`: it has no `__del__` at any
    /// depth and owns nothing else. Freeing a string runs no user code, so
    /// doing them all at once is indistinguishable from the field walk.
    pub(crate) only_strings: bool,
    /// The record holds no addresses into its own bytes, so moving them
    /// needs no repair.
    pub(crate) fixed: bool,
}
const _: () = {
    assert!(size_of::<Type>() == 80);
    assert!(std::mem::offset_of!(Type, plan) == 72);
    assert!(size_of::<Plan>() == 24);
    assert!(std::mem::offset_of!(Plan, only_strings) == 16);
    assert!(std::mem::offset_of!(Plan, fixed) == 17);
    assert!(std::mem::offset_of!(Type, key) == 8);
    assert!(std::mem::offset_of!(Type, name) == 24);
    assert!(std::mem::offset_of!(Type, variants) == 40);
    assert!(std::mem::offset_of!(Type, drop) == 56);
    assert!(std::mem::offset_of!(Type, wrapper) == 64);
    assert!(size_of::<Variant>() == 32);
};
impl Type {
    fn managed(&self) -> bool {
        self.kind == b'B'
            || matches!(
                self.kind,
                b's' | b'L'
                    | b'Q'
                    | b'S'
                    | b'D'
                    | b'E'
                    | b'C'
                    | b'G'
                    | b'F'
                    | b'A'
                    | b'I'
                    | b'M'
                    | b'U'
                    | b'X'
                    | b'Y'
                    | b'P'
                    | b'Z'
                    | b'K'
                    | b'O'
                    | b'J'
            )
    }
    /// True when `release_next` would only clear the slot: no heap owner,
    /// record, sum payload, closure environment or inline generator.
    fn owns_nothing(&self) -> bool {
        !self.managed() && self.kind != b'H'
    }
    /// A class record whose bytes can move without repair; see [`Plan::fixed`].
    pub(crate) fn fixed(&self) -> bool {
        self.plan.is_some_and(|plan| plan.fixed)
    }
    /// The strings a record of this class owns, when they are all it owns.
    fn only_strings(&self) -> Option<&'static [u32]> {
        self.plan
            .filter(|plan| plan.only_strings)
            .map(|plan| plan.strings)
    }
    fn key(&self) -> &Type {
        self.key.expect("collection key type")
    }
    fn value(&self) -> &Type {
        self.value.expect("dictionary value type")
    }
    pub(crate) fn tag(&self, value: u128) -> usize {
        let mask = (self.variants.len().next_power_of_two() - 1).max(1);
        (value >> 64) as usize & mask
    }
    /// The single field stored directly in an inline sum's payload word.
    pub(crate) fn payload(&self, value: u128) -> Option<&Type> {
        match self.variants[self.tag(value)].fields {
            [field] => Some(field),
            _ => None,
        }
    }
    /// The owner-local field storage of a class or a multi-field variant.
    pub(crate) fn record(&self, value: u128) -> Option<Record<'_>> {
        let fields = match self.kind {
            b'C' => Fields::Class(self.variants),
            b'B' => match self.variants[self.tag(value)].fields {
                fields @ [_, _, ..] => Fields::Variant(fields),
                _ => return None,
            },
            _ => return None,
        };
        let base = value as u64 as *mut u128;
        let state = self.kind == b'C' && self.drop.is_some();
        (!base.is_null()).then_some(Record {
            fields,
            base,
            state,
        })
    }
    fn tag_bits(&self) -> u32 {
        self.variants.len().next_power_of_two().ilog2().max(1)
    }
    pub(crate) fn unpack(&self, value: u128) -> u128 {
        (value & u64::MAX as u128) | ((value >> (64 + self.tag_bits())) << 64)
    }
    pub(crate) fn pack(&self, value: u128, tag: usize) -> u128 {
        (value & u64::MAX as u128) | ((((value >> 64) << self.tag_bits()) | tag as u128) << 64)
    }
}
#[cfg(test)]
pub(crate) fn payload(value: u128) -> u128 {
    (value & u64::MAX as u128) | ((value >> 65) << 64)
}
pub(crate) fn wrap(value: u128, tag: u64) -> u128 {
    (value & u64::MAX as u128) | (((value >> 64) * 2 + tag as u128) << 64)
}

#[repr(C)]
struct Collection {
    header: Header,
    ty: *const Type,
    entries: Entries,
    table: Vec<usize>,
    /// The live row to resume after a heap child finishes destruction.
    drop_entry: Option<usize>,
}
const _: () = {
    use crate::list_layout::{BYTES, REMOVED, ROWS};
    assert!(std::mem::offset_of!(Collection, ty) == 16);
    // `Entries` checks its own field offsets; these add the owner header.
    assert!(std::mem::offset_of!(Collection, entries) == ROWS as usize);
    assert!(ROWS + 8 == BYTES && ROWS + 24 == REMOVED);
};

#[derive(Clone, Copy)]
pub(crate) enum Fields<'a> {
    /// Class descriptors name each field as a one-field variant.
    Class(&'a [Variant]),
    Variant(&'a [&'static Type]),
}
/// Typed field slots inside their owner's storage. Each slot reserves its
/// type's complete inline payload. A field of a class, tuple or multi-field
/// variant whose type is a class with a record is flat instead: its record's bytes sit at the field's offset, with no value
/// word (see [`Type::flat_field`]). A class with `__del__` keeps one state
/// word before its fields: bit 0 marks a completed initializer, and the
/// remaining bits count observers, which borrow the instance through a
/// retained copy of its value word and must not run its destructor.
#[derive(Clone, Copy)]
pub(crate) struct Record<'a> {
    fields: Fields<'a>,
    /// The start of the storage, including any state word.
    base: *mut u128,
    state: bool,
}
impl Record<'_> {
    pub(crate) fn len(&self) -> usize {
        match self.fields {
            Fields::Class(fields) => fields.len(),
            Fields::Variant(fields) => fields.len(),
        }
    }
    pub(crate) fn ty(&self, index: usize) -> &'static Type {
        match self.fields {
            Fields::Class(fields) => fields[index].fields[0],
            Fields::Variant(fields) => fields[index],
        }
    }
    /// Storage size in 16-byte words, including any state word.
    pub(crate) fn words(&self) -> usize {
        usize::from(self.state)
            + (0..self.len())
                .map(|i| self.width(self.ty(i)))
                .sum::<usize>()
    }
    /// Class records, tuples and multi-field variants all store class fields
    /// with records flat.
    fn flat(&self, ty: &Type) -> bool {
        ty.flat_field()
    }
    fn width(&self, ty: &Type) -> usize {
        if self.flat(ty) {
            ty.field_words()
        } else {
            ty.slot_words()
        }
    }
    pub(crate) fn moved_to(&self, base: *mut u128) -> Self {
        Self { base, ..*self }
    }
    /// Visit fields in declaration order with one width lookup per field.
    /// Raw pointers avoid borrowing payloads that callers may move or release.
    pub(crate) fn slots(&self) -> impl Iterator<Item = (Field, &'static Type)> + '_ {
        (0..self.len()).scan(usize::from(self.state), move |offset, index| {
            let ty = self.ty(index);
            // SAFETY: the owner reserved `words()` words at `base`.
            let at = unsafe { self.base.add(*offset) };
            let flat = self.flat(ty);
            *offset += if flat {
                ty.field_words()
            } else {
                ty.slot_words()
            };
            Some((Field { at, flat }, ty))
        })
    }
    /// The record's storage, as its slot would address it: a flat record has
    /// no slot, so this is the virtual slot just before its bytes, which is
    /// what a reference to the instance addresses.
    fn receiver(&self) -> *mut u128 {
        self.base.wrapping_sub(1)
    }
    fn state(&self) -> Option<*mut u64> {
        self.state.then_some(self.base.cast())
    }
    /// Return whether this owner must release the fields. Observers only consume
    /// their count; a live owner's hook is disarmed before calling user code.
    unsafe fn begin_release(&self, ty: &Type) -> bool {
        unsafe {
            if let Some(state) = self.state() {
                if *state >= 2 {
                    *state -= 2;
                    return false;
                }
                if *state & 1 != 0 {
                    *state = 0;
                    let hook = ty.drop.expect("class destructor");
                    let _caller = crate::CallerScope::save();
                    // `self` addresses the record through its (possibly
                    // virtual) slot; compiled code never reads or writes a
                    // class slot's word through a reference.
                    memory::with_nested_drops(|| hook(self.receiver()));
                }
            }
            true
        }
    }
    /// Release the owned fields in declaration order. Inline fields finish
    /// immediately; heap fields join the destruction queue.
    unsafe fn release(&self) {
        for (field, ty) in self.slots() {
            unsafe { release(field.read(), ty) };
        }
    }
}

/// A field's place in its owner's record: a slot, or a flat class record.
#[derive(Clone, Copy)]
pub(crate) struct Field {
    /// The slot, or the first byte of a flat field's record.
    pub(crate) at: *mut u128,
    pub(crate) flat: bool,
}
impl Field {
    /// The record of a flat field of class type `ty`, without the checks
    /// [`Type::record`] makes for a value word.
    pub(crate) fn record(self, ty: &'static Type) -> Record<'static> {
        debug_assert!(self.flat && ty.flat_field());
        Record {
            fields: Fields::Class(ty.variants),
            base: self.at,
            state: ty.drop.is_some(),
        }
    }
    /// The field's value: a flat record's value is its address.
    pub(crate) unsafe fn read(self) -> u128 {
        if self.flat {
            self.at as u64 as u128
        } else {
            unsafe { self.at.read() }
        }
    }
    /// Store `value`, of type `ty`, whose inline payload is already in place.
    /// A flat field keeps no word, so only an empty value changes it: zero
    /// bytes own nothing.
    pub(crate) unsafe fn write(self, value: u128, ty: &Type) {
        unsafe {
            if !self.flat {
                self.at.write(value);
            } else if value == 0 {
                self.clear(ty);
            } else {
                debug_assert_eq!(value as u64, self.at as u64, "flat field moved");
            }
        }
    }
    /// Whether the field owns nothing: an empty slot, or a flat record whose
    /// bytes are all zero (moved out, or already released).
    pub(crate) unsafe fn is_clear(self, ty: &Type) -> bool {
        unsafe {
            if self.flat {
                std::slice::from_raw_parts(self.at, ty.payload_bytes() / 16)
                    .iter()
                    .all(|word| *word == 0)
            } else {
                self.at.read() == 0
            }
        }
    }
    /// Leave the field empty.
    pub(crate) unsafe fn clear(self, ty: &Type) {
        unsafe {
            if self.flat {
                self.at.write_bytes(0, ty.payload_bytes() / 16);
            } else {
                self.at.write(0);
            }
        }
    }
}

/// Boxes are the only heap records: recursive types reach themselves through them.
#[repr(C)]
struct Boxed {
    header: Header,
    ty: *const Type,
    padding: u64,
    slot: [u128; 0],
}
const _: () = assert!(std::mem::offset_of!(Boxed, slot) == 32);

/// Move `value` into a new box. On failure the caller still owns `value`.
unsafe fn try_box_new(ty: &Type, value: u128) -> Result<*mut Boxed, AllocError> {
    let content = ty.key();
    let boxed = memory::try_allocate::<Boxed, u128>(content.slot_words())?;
    unsafe {
        boxed.write(Boxed {
            header: Header::new(box_destroy),
            ty,
            padding: 0,
            slot: [],
        });
        ranges::store(std::ptr::addr_of_mut!((*boxed).slot).cast(), value, content);
    }
    Ok(boxed)
}
unsafe extern "C" fn box_destroy(header: *mut Header) {
    unsafe {
        let boxed = header.cast::<Boxed>();
        let content = (*(*boxed).ty).key();
        let slot = std::ptr::addr_of_mut!((*boxed).slot).cast::<u128>();
        if let Some(child) = release_next(slot, content) {
            memory::continue_after(header, child);
            return;
        }
        memory::free::<Boxed, u128>(boxed, content.slot_words());
    }
}
/// Free a box whose content the caller has already moved out.
unsafe fn box_take(boxed: *mut Boxed) {
    unsafe {
        std::ptr::addr_of_mut!((*boxed).slot)
            .cast::<u128>()
            .write(0);
        plenty_release(boxed.cast());
    }
}

unsafe fn value_type<'a>(value: u128) -> &'a Type {
    // SAFETY: all managed aggregates have an immutable static Type pointer at byte 16.
    unsafe {
        &**(value as *const u8)
            .add(size_of::<Header>())
            .cast::<*const Type>()
    }
}
pub(crate) unsafe fn retain(value: u128, ty: &Type) {
    unsafe {
        if ty.kind == b'H' {
            crate::closures::retain(value as *mut u128);
        } else if let Some(record) = ty.record(value) {
            if let Some(state) = record.state() {
                *state += 2;
                return;
            }
            for (field, ty) in record.slots() {
                retain(field.read(), ty);
            }
        } else if ty.kind == b'B' {
            if let Some(t) = ty.payload(value) {
                retain(ty.unpack(value), t);
            }
        } else if ty.kind == b'C' {
        } else if ty.managed() {
            plenty_retain(value as *mut Header);
        }
    }
}
pub(crate) unsafe fn release(value: u128, ty: &Type) {
    unsafe {
        if ty.kind == b'H' {
            crate::closures::release(value as *mut u128);
        } else if let Some(strings) = ty.only_strings()
            && value as u64 != 0
        {
            release_strings(value as u64 as *mut u128, strings);
        } else if let Some(record) = ty.record(value) {
            if record.begin_release(ty) {
                record.release();
            }
        } else if ty.kind == b'B' {
            if let Some(t) = ty.payload(value) {
                release(ty.unpack(value), t);
            }
        } else if ty.kind == b'G' && ty.inline_bytes != 0 {
            crate::generators::release_inline(value as *mut Generator);
        } else if ty.managed() {
            plenty_release(value as *mut Header);
        }
    }
}

/// Free the strings of a record at `base`; see [`Plan::only_strings`].
unsafe fn release_strings(base: *mut u128, strings: &[u32]) {
    for &word in strings {
        // SAFETY: the plan's offsets lie inside the record.
        let text = unsafe { base.add(word as usize).read() } as u64;
        // A moved-out field is clear.
        if text != 0 {
            unsafe { strings::release_leaf(text as *mut Text) };
        }
    }
}

/// Consume inline storage up to its next heap owner, leaving the remaining
/// fields live for the enclosing heap object's next destruction callback.
/// Cleared slots and the class initializer bit remember completed work without
/// allocating a traversal stack. Recursion follows only finite inline layouts;
/// every heap edge returns to the intrusive queue before descending further.
unsafe fn release_next(slot: *mut u128, ty: &Type) -> Option<*mut Header> {
    unsafe {
        let value = slot.read();
        if value == 0 {
            return None;
        }
        if ty.kind == b'H' {
            let environment = value as *mut u128;
            let descriptor = &**environment.cast::<*const Type>();
            let fields = descriptor.variants[0].fields;
            let mut capture =
                environment.add(1 + fields.iter().map(|t| t.slot_words()).sum::<usize>());
            for field in fields.iter().rev() {
                capture = capture.sub(field.slot_words());
                if let Some(child) = release_next(capture, field) {
                    return Some(child);
                }
            }
        } else if let Some(strings) = ty.only_strings()
            && value as u64 != 0
        {
            release_strings(value as u64 as *mut u128, strings);
        } else if let Some(record) = ty.record(value) {
            if !record.begin_release(ty) {
                slot.write(0);
                return None;
            }
            if let Some(child) = release_next_fields(record) {
                return Some(child);
            }
        } else if ty.kind == b'B' {
            if let Some(payload_type) = ty.payload(value) {
                let mut payload = ty.unpack(value);
                let child = release_next(&mut payload, payload_type);
                slot.write(ty.pack(payload, ty.tag(value)));
                if child.is_some() {
                    return child;
                }
            }
        } else if ty.kind == b'G' && ty.inline_bytes != 0 {
            crate::generators::release_inline(value as *mut Generator);
        } else if ty.kind == b's' {
            // A string owns nothing and runs no user code, so freeing it now
            // is unobservable and spares the owner a restart of its walk.
            slot.write(0);
            strings::release_leaf(value as *mut Text);
            return None;
        } else if ty.managed() {
            slot.write(0);
            return Some(value as *mut Header);
        }
        slot.write(0);
        None
    }
}

/// `release_next` for each field of a record. A flat field's progress is
/// recorded in its own fields, which are cleared as they finish: a record of
/// zero bytes owns nothing.
unsafe fn release_next_fields(record: Record<'_>) -> Option<*mut Header> {
    unsafe {
        for (field, ty) in record.slots() {
            // A walk resumes from the first field after each heap child, so
            // skip finished flat records without descending into them.
            let child = if field.flat {
                if field.is_clear(ty) {
                    continue;
                }
                if let Some(strings) = ty.only_strings() {
                    release_strings(field.at, strings);
                    field.clear(ty);
                    continue;
                }
                let nested = field.record(ty);
                if nested.begin_release(ty) {
                    let child = release_next_fields(nested);
                    if child.is_none() {
                        // Inline payload bytes outlive their slots' words.
                        field.clear(ty);
                    }
                    child
                } else {
                    // An observer's count was consumed: forget the fields,
                    // as clearing a slot does.
                    field.clear(ty);
                    None
                }
            } else {
                release_next(field.at, ty)
            };
            if child.is_some() {
                return child;
            }
        }
        None
    }
}

fn try_collection_new(ty: &'static Type, capacity: usize) -> Result<*mut Collection, AllocError> {
    let mut collection = Collection {
        header: Header::new(collection_destroy),
        ty,
        entries: if ty.kind == b'L' {
            Entries::new(ty.key(), ty.value)
        } else {
            Entries::ordered(ty.key(), ty.value)
        },
        table: Vec::new(),
        drop_entry: None,
    };
    // Validate and reserve before publishing an owner. Until the header is
    // allocated, ordinary Rust drops reclaim these empty buffers on any error.
    // No payloads exist yet and no user destructor can run on a partial object.
    unsafe {
        collection.try_reserve(capacity)?;
    }
    let pointer = memory::try_allocate::<Collection, u8>(0)?;
    unsafe {
        pointer.write(collection);
    }
    Ok(pointer)
}
unsafe extern "C" fn collection_destroy(header: *mut Header) {
    unsafe {
        let c = header.cast::<Collection>();
        let ty = &*(*c).ty;
        // A list of values that own nothing needs no per-row release; skipping
        // the walk avoids touching every row before the buffer is freed.
        let mut entry = if ty.kind == b'L'
            && (ty.key().owns_nothing() || ty.key().only_strings().is_some_and(<[u32]>::is_empty))
        {
            None
        } else {
            (*c).drop_entry.or_else(|| (*c).entries.indices().next())
        };
        while let Some(index) = entry {
            (*c).drop_entry = Some(index);
            if let Some(child) = release_next((*c).entries.slot(index, false), ty.key()) {
                memory::continue_after(header, child);
                return;
            }
            if let Some(value) = ty.value
                && let Some(child) = release_next((*c).entries.slot(index, true), value)
            {
                memory::continue_after(header, child);
                return;
            }
            entry = if ty.kind == b'L' {
                (index + 1 < (*c).entries.len()).then_some(index + 1)
            } else {
                (*c).entries.next(index)
            };
        }
        std::ptr::drop_in_place(c);
        memory::free::<Collection, u8>(c, 0);
    }
}
/// Build the endpoint pair directly in the caller's two-slot tuple storage.
pub(crate) unsafe fn channel_new(
    result: &'static Type,
    capacity: usize,
    storage: *mut u128,
) -> u128 {
    if capacity == 0 {
        return wrap(wrap(0, 0), 1); // Err(ChannelError.InvalidCapacity)
    }
    unsafe {
        let pair = result.variants[0].fields[0];
        match crate::channels::create(pair.variants[0].fields[0].key(), capacity) {
            Ok((sender, receiver)) => {
                storage.write(sender);
                storage.add(1).write(receiver);
                wrap(pair.pack(storage as u128, 0), 0)
            }
            Err(error) => wrap(wrap(wrap(0, error as u64), 1), 1),
        }
    }
}
pub(crate) fn index(index: i64, len: usize, site: *const crate::Site) -> usize {
    checked_index(index, len).unwrap_or_else(|| out_of_bounds(index, len, site))
}

/// Reports the index as written, before negative-index normalization.
#[cold]
pub(crate) fn out_of_bounds(index: i64, len: usize, site: *const crate::Site) -> ! {
    crate::fail_kind_at(
        crate::actor_heap::TrapKind::Index,
        site,
        format_args!("index out of bounds: index {index}, length {len}"),
    )
}

/// Python-style forward slice bounds, without overflow at extreme signed inputs.
pub(crate) fn slice_bounds(start: i64, stop: i64, len: usize) -> std::ops::Range<usize> {
    let normalize = |index: i64| {
        let index = index as i128;
        let index = if index < 0 {
            len as i128 + index
        } else {
            index
        };
        index.clamp(0, len as i128) as usize
    };
    let start = normalize(start);
    start..normalize(stop).max(start)
}

pub(crate) fn checked_index(index: i64, len: usize) -> Option<usize> {
    let i = if index < 0 {
        index as i128 + len as i128
    } else {
        index as i128
    };
    (i >= 0 && i < len as i128).then_some(i as usize)
}
unsafe fn hash(value: u128, ty: &Type) -> u64 {
    // SAFETY: the caller supplies a live key matching its descriptor.
    unsafe { hashing::hash(value, ty) }
}
impl Collection {
    fn ty(&self) -> &Type {
        unsafe { &*self.ty }
    }
    fn len(&self) -> usize {
        self.entries.len()
    }
    unsafe fn bucket(&self, key: u128) -> usize {
        unsafe {
            let mut bucket = hash(key, self.ty().key()) as usize & (self.table.len() - 1);
            while self.table[bucket] != 0
                && !equal(
                    self.entries.get(self.table[bucket] - 1).key,
                    key,
                    self.ty().key(),
                )
            {
                bucket = (bucket + 1) & (self.table.len() - 1);
            }
            bucket
        }
    }
    unsafe fn find(&self, key: u128) -> Option<usize> {
        if self.table.is_empty() {
            None
        } else {
            unsafe { self.table[self.bucket(key)].checked_sub(1) }
        }
    }
    /// Reserve all buffers before publishing a new hash index. On error,
    /// logical contents and the old index remain intact; capacity may change.
    unsafe fn try_reserve(&mut self, additional: usize) -> Result<(), AllocError> {
        let needed = self
            .entries
            .len()
            .checked_add(additional)
            .filter(|n| *n <= i64::MAX as usize)
            .ok_or(AllocError::CapacityOverflow)?;
        memory::checked_capacity::<Entry>(needed)?;
        let table_size = if self.ty().kind != b'L' && needed > self.table.len() / 2 {
            let count = needed
                .checked_mul(2)
                .and_then(|n| n.max(16).checked_next_power_of_two())
                .ok_or(AllocError::CapacityOverflow)?;
            memory::checked_capacity::<usize>(count)?;
            count
        } else {
            0
        };
        let mut table = Vec::new();
        if table_size != 0 {
            memory::try_reserve(&mut table, table_size)?;
            table.resize(table_size, 0);
        }
        self.entries.try_reserve(additional)?;
        if table_size != 0 {
            // Hashing legal keys (integers, bool, str) never allocates or calls
            // user code. Build from hashes alone: all existing keys are unique.
            for i in self.entries.indices() {
                let entry = self.entries.get(i);
                let mut bucket =
                    unsafe { hash(entry.key, self.ty().key()) } as usize & (table_size - 1);
                while table[bucket] != 0 {
                    bucket = (bucket + 1) & (table_size - 1);
                }
                table[bucket] = i + 1;
            }
            self.table = table;
        }
        Ok(())
    }

    unsafe fn insert(&mut self, key: u128, value: u128) {
        unsafe { self.try_insert(key, value) }
            .unwrap_or_else(|_| crate::fail("collection allocation failed"));
    }

    /// Source ownership transfers only after reservation. On error the caller
    /// still owns both unchanged collections and handles source cleanup.
    unsafe fn try_extend(&mut self, source: &mut Collection) -> Result<(), AllocError> {
        unsafe {
            self.try_reserve(source.entries.len())?;
        }
        unsafe {
            self.entries.append(&mut source.entries);
        }
        Ok(())
    }

    /// Count missing keys before reserving. No logical update or payload cleanup
    /// occurs until entry, order, and bucket capacity are available.
    unsafe fn try_update(&mut self, source: &mut Collection) -> Result<(), AllocError> {
        unsafe {
            let additional = source
                .entries
                .iter()
                .filter(|entry| self.find(entry.key).is_none())
                .count();
            self.try_reserve(additional)?;
            let ty = &*self.ty;
            source.table.fill(0);
            for entry in source.entries.iter() {
                if let Some(index) = self.find(entry.key) {
                    // An inline value lives in the row, so release it before
                    // its storage is overwritten.
                    if let Some(value_type) = ty.value {
                        release(self.entries.get(index).value, value_type);
                    }
                    self.entries.set(index, true, entry.value);
                    release(entry.key, ty.key());
                } else {
                    let bucket = self.bucket(entry.key);
                    self.table[bucket] = self.entries.vacant() + 1;
                    self.entries.push(entry);
                }
            }
            source.entries.clear();
            Ok(())
        }
    }

    /// Store borrowed copyable operands and adopt affine ones, which move
    /// in. A failed insertion drops what it adopted; borrowed operands stay
    /// with the caller.
    unsafe fn try_insert(&mut self, key: u128, value: u128) -> Result<(), AllocError> {
        unsafe {
            // The descriptor is static; reading it does not borrow `self`.
            let ty = &*self.ty;
            let key_ty = ty.key();
            let value_ty = ty.value;
            if !key_ty.affine {
                retain(key, key_ty);
            }
            if let Some(ty) = value_ty.filter(|ty| !ty.affine) {
                retain(value, ty);
            }
            let stored = self.try_store(key, value);
            if stored.is_err() {
                release(key, key_ty);
                if let Some(ty) = value_ty {
                    release(value, ty);
                }
            }
            stored
        }
    }
    /// Insert owned operands without adjusting their ownership.
    unsafe fn try_store(&mut self, key: u128, value: u128) -> Result<(), AllocError> {
        unsafe {
            if self.ty().kind != b'L'
                && let Some(i) = self.find(key)
            {
                if let Some(ty) = &self.ty().value {
                    release(self.entries.get(i).value, ty);
                }
                release(key, self.ty().key());
                self.entries.set(i, true, value);
                return Ok(());
            }
            let additional = if self.entries.len() == self.entries.capacity() {
                // Geometric growth without making explicit reserve speculative.
                self.entries
                    .len()
                    .max(4)
                    .min(
                        (isize::MAX as usize / self.entries.row_bytes())
                            .saturating_sub(self.entries.len()),
                    )
                    .max(1)
            } else {
                1
            };
            self.try_reserve(additional)?;
            if self.ty().kind != b'L' {
                let bucket = self.bucket(key);
                self.table[bucket] = self.entries.vacant() + 1;
            }
            self.entries.push(Entry { key, value });
            Ok(())
        }
    }
    /// Unlink the stable entry slot and repair only its linear-probe cluster.
    /// Dictionary payload owners are transferred, never retained or released.
    /// Sets have a zero payload; Some(0) still distinguishes removal from a miss.
    unsafe fn remove(&mut self, key: u128) -> Option<u128> {
        unsafe {
            if self.table.is_empty() {
                return None;
            }
            let mut hole = self.bucket(key);
            let index = self.table[hole].checked_sub(1)?;
            let entry = self.entries.remove(index);
            let mask = self.table.len() - 1;
            let mut probe = (hole + 1) & mask;
            while self.table[probe] != 0 {
                let slot = self.table[probe] - 1;
                let home = hash(self.entries.get(slot).key, self.ty().key()) as usize & mask;
                // Move only references whose circular probe path crosses the
                // hole. An empty bucket terminates even across wraparound.
                if (hole.wrapping_sub(home) & mask) < (probe.wrapping_sub(home) & mask) {
                    self.table[hole] = self.table[probe];
                    hole = probe;
                }
                probe = (probe + 1) & mask;
            }
            self.table[hole] = 0;
            release(entry.key, self.ty().key());
            Some(entry.value)
        }
    }

    unsafe fn at(&self, index: usize) -> u128 {
        unsafe {
            let slot = if self.ty().kind == b'L' {
                index
            } else {
                self.entries.indices().nth(index).expect("entry index")
            };
            let value = self.entries.get(slot).key;
            retain(value, self.ty().key());
            value
        }
    }
}

/// Structural equality. Every value has one owner, so traversal is bounded by
/// the value's own size; nothing is shared except immutable strings.
unsafe fn equal(a: u128, b: u128, ty: &Type) -> bool {
    unsafe {
        if a == b && ty.reflexive {
            return true;
        }
        match ty.kind {
            b'1' | b'5' | b'b' => a as u8 == b as u8,
            b'2' | b'6' => a as u16 == b as u16,
            b'3' | b'7' => a as u32 == b as u32,
            b'4' | b'8' => a as u64 == b as u64,
            b'B' | b'C' => {
                if ty.kind == b'B' && ty.tag(a) != ty.tag(b) {
                    return false;
                }
                match (ty.record(a), ty.record(b)) {
                    (Some(left), Some(right)) => left
                        .slots()
                        .zip(right.slots())
                        .all(|((a, ty), (b, _))| equal(a.read(), b.read(), ty)),
                    (None, None) => ty
                        .payload(a)
                        .is_none_or(|t| equal(ty.unpack(a), ty.unpack(b), t)),
                    _ => false,
                }
            }
            b'f' => f32::from_bits(a as u32) == f32::from_bits(b as u32),
            b'd' => f64::from_bits(a as u64) == f64::from_bits(b as u64),
            b's' | b'y' => {
                strings::bytes(&(a as *const Text)) == strings::bytes(&(b as *const Text))
            }
            b'U' => byte_storage::equal(a, b),
            b'Q' => clist::equal(a, b, ty),
            b'O' => {
                let (a, b) = (a as *const Boxed, b as *const Boxed);
                let slot =
                    |boxed: *const Boxed| std::ptr::addr_of!((*boxed).slot).cast::<u128>().read();
                equal(slot(a), slot(b), ty.key())
            }
            b'R' => {
                let (a, b) = (&*(a as *const Range), &*(b as *const Range));
                a.len == b.len
                    && (a.len == 0 || (a.start == b.start && (a.len == 1 || a.step == b.step)))
            }
            b'L' | b'S' | b'D' => {
                let (a, b) = (&*(a as *const Collection), &*(b as *const Collection));
                if a.len() != b.len() {
                    return false;
                }
                for (i, entry) in a.entries.iter().enumerate() {
                    if ty.kind == b'L' {
                        if !equal(entry.key, b.entries.get(i).key, ty.key()) {
                            return false;
                        }
                    } else {
                        let Some(j) = b.find(entry.key) else {
                            return false;
                        };
                        if ty.kind == b'D'
                            && !equal(entry.value, b.entries.get(j).value, ty.value())
                        {
                            return false;
                        }
                    }
                }
                true
            }
            _ => a == b,
        }
    }
}
/// The total order `sort` uses, defined for integers, floats, strings, and
/// tuples of those. Values that compare equal are indistinguishable apart from
/// NaN payloads, so an unstable sort has one observable result.
unsafe fn compare(a: u128, b: u128, ty: &Type) -> Ordering {
    // `<` leaves NaN unordered and equates the zeros. Sorting needs every value
    // placed: zeros order by sign and every NaN follows the numbers.
    fn float(a: f64, b: f64) -> Ordering {
        match (a.is_nan(), b.is_nan()) {
            (false, false) => a.total_cmp(&b),
            (a, b) => a.cmp(&b),
        }
    }
    unsafe {
        match ty.kind {
            b'1' => (a as i8).cmp(&(b as i8)),
            b'2' => (a as i16).cmp(&(b as i16)),
            b'3' => (a as i32).cmp(&(b as i32)),
            b'4' => (a as i64).cmp(&(b as i64)),
            b'5' => (a as u8).cmp(&(b as u8)),
            b'6' => (a as u16).cmp(&(b as u16)),
            b'7' => (a as u32).cmp(&(b as u32)),
            b'8' => (a as u64).cmp(&(b as u64)),
            b'f' => float(
                f32::from_bits(a as u32).into(),
                f32::from_bits(b as u32).into(),
            ),
            b'd' => float(f64::from_bits(a as u64), f64::from_bits(b as u64)),
            // UTF-8 byte order is code point order.
            b's' => strings::bytes(&(a as *const Text)).cmp(strings::bytes(&(b as *const Text))),
            b'B' => match (ty.record(a), ty.record(b)) {
                (Some(left), Some(right)) => left
                    .slots()
                    .zip(right.slots())
                    .map(|((a, ty), (b, _))| compare(a.read(), b.read(), ty))
                    .find(|order| order.is_ne())
                    .unwrap_or(Ordering::Equal),
                _ => ty.payload(a).map_or(Ordering::Equal, |field| {
                    compare(ty.unpack(a), ty.unpack(b), field)
                }),
            },
            _ => crate::fail("sort does not support this element type"),
        }
    }
}
/// A tuple of scalars and strings, with each field's word offset resolved once
/// per sort rather than walking the descriptor on every comparison.
struct FlatTuple {
    fields: [(usize, &'static Type); FlatTuple::MAX],
    len: usize,
}
impl FlatTuple {
    const MAX: usize = 8;

    fn new(ty: &Type) -> Option<Self> {
        if ty.kind != b'B' || ty.variants.len() != 1 {
            return None;
        }
        let variant = ty.variants[0].fields;
        if !(2..=Self::MAX).contains(&variant.len()) {
            return None;
        }
        let mut fields = [(0, variant[0]); Self::MAX];
        let mut offset = 0;
        for (field, slot) in variant.iter().zip(&mut fields) {
            if !matches!(field.kind, b'1'..=b'8' | b'f' | b'd' | b's') {
                return None;
            }
            *slot = (offset, *field);
            offset += field.slot_words();
        }
        Some(Self {
            fields,
            len: variant.len(),
        })
    }

    /// The same order as `compare`.
    unsafe fn compare(&self, a: u128, b: u128, ty: &Type) -> Ordering {
        let (left, right) = (a as u64 as *const u128, b as u64 as *const u128);
        unsafe {
            if left.is_null() || right.is_null() {
                return compare(a, b, ty);
            }
            for &(offset, field) in &self.fields[..self.len] {
                let order = compare(left.add(offset).read(), right.add(offset).read(), field);
                if order.is_ne() {
                    return order;
                }
            }
        }
        Ordering::Equal
    }
}

/// Copy `value` into an independent owner. A value with inline storage first
/// moves its bytes to `storage`, which reserves `ty.payload_bytes()`.
unsafe fn try_copy(value: u128, ty: &Type, storage: *mut u128) -> Result<u128, AllocError> {
    unsafe {
        let mut value = if ty.payload_bytes() != 0 {
            ranges::copy_payload(value, ty, storage.cast())
        } else {
            value
        };
        try_copy_into(&mut value, ty)?;
        Ok(value)
    }
}

/// A fully initialized temporary owner, including a partially filled collection.
/// It never describes a record whose fields are still being initialized.
struct OwnedValue<'a> {
    value: u128,
    ty: &'a Type,
}
impl OwnedValue<'_> {
    fn into_value(self) -> u128 {
        let value = self.value;
        std::mem::forget(self);
        value
    }
}
impl Drop for OwnedValue<'_> {
    fn drop(&mut self) {
        // SAFETY: this guard owns one live value with its matching type.
        unsafe {
            release(self.value, self.ty);
        }
    }
}

/// Copy borrowed UTF-8 into an owned list, guarding every initialized prefix.
pub(crate) unsafe fn try_text_list<'a>(
    texts: impl ExactSizeIterator<Item = Result<&'a str, crate::text_io::Error>>,
    ty: &'static Type,
) -> Result<u128, crate::text_io::Error> {
    unsafe {
        let output = try_collection_new(ty, texts.len())?;
        let owner = OwnedValue {
            value: output as u128,
            ty,
        };
        for text in texts {
            let text = OwnedValue {
                value: strings::try_new(text?)? as u128,
                ty: ty.key(),
            };
            (*output).try_insert(text.value, 0)?;
        }
        Ok(owner.into_value())
    }
}

pub(crate) unsafe fn try_reader_lines(
    reader: &mut impl std::io::Read,
    skip_lf: &mut bool,
    ty: &'static Type,
) -> Result<u128, crate::text_io::Error> {
    // SAFETY: the caller provides valid list[str] metadata.
    // Each temporary text and the initialized list prefix have an owning guard.
    unsafe {
        let output = try_collection_new(ty, 0)?;
        let owner = OwnedValue {
            value: output as u128,
            ty,
        };
        loop {
            let line = OwnedValue {
                value: crate::text_io::read_file_line(reader, skip_lf)? as u128,
                ty: ty.key(),
            };
            if strings::utf8(&(line.value as *const Text)).is_empty() {
                break;
            }
            (*output).try_insert(line.value, 0)?;
        }
        Ok(owner.into_value())
    }
}

unsafe fn write_file_lines(
    file: *mut crate::files::File,
    lines: *const Collection,
) -> Result<u128, crate::text_io::Error> {
    // SAFETY: the compiler retains a shared list[str] loan and exclusive File
    // access. No callbacks or mutations of the list occur during these writes.
    unsafe {
        // Validate state even for an empty list; an empty write preserves CRLF state.
        crate::files::write(file, "")?;
        for entry in (*lines).entries.iter() {
            crate::files::write(file, strings::utf8(&(entry.key as *const Text)))?;
        }
    }
    Ok(0)
}

/// Count the selected unique members before allocating. Inputs stay borrowed;
/// output members retain immutable keys only after all storage is reserved.
unsafe fn try_set_from_entries(
    entries: impl Iterator<Item = Entry> + Clone,
    ty: &'static Type,
) -> Result<u128, AllocError> {
    unsafe {
        let output = try_collection_new(ty, entries.clone().count())?;
        let guard = OwnedValue {
            value: output as u128,
            ty,
        };
        for entry in entries {
            // Capacity covers every selected member; this cannot grow storage.
            (*output).try_insert(entry.key, 0)?;
        }
        Ok(guard.into_value())
    }
}

/// Inputs stay borrowed throughout both passes. The output guard releases the
/// initialized prefix on failure, without allocating or invoking user code.
unsafe fn try_split(
    text: *const Text,
    separator: *const Text,
    ty: &'static Type,
    site: *const crate::Site,
) -> Result<u128, AllocError> {
    unsafe {
        let separator = strings::utf8(&separator);
        if separator.is_empty() {
            crate::fail_at(
                site,
                format_args!("string split requires a nonempty separator"),
            );
        }
        let pieces = strings::utf8(&text).split(separator);
        try_text_pieces(pieces, ty)
    }
}

/// Borrow pieces during two passes, owning every output and its partial prefix.
unsafe fn try_text_pieces<'a>(
    pieces: impl Iterator<Item = &'a str> + Clone,
    ty: &'static Type,
) -> Result<u128, AllocError> {
    unsafe {
        let result = try_collection_new(ty, pieces.clone().count())?;
        let owner = OwnedValue {
            value: result as u128,
            ty,
        };
        for piece in pieces {
            let piece = OwnedValue {
                value: strings::try_new(piece)? as u128,
                ty: ty.key(),
            };
            (*result).try_insert(piece.value, 0)?;
        }
        Ok(owner.into_value())
    }
}

/// Reserve the complete snapshot before retaining or transferring any elements.
/// Affine values require a unique temporary source; the frontend enforces this.
unsafe fn try_dictionary_snapshot(
    source: &mut Collection,
    ty: &'static Type,
    values: bool,
) -> Result<u128, AllocError> {
    unsafe {
        let result = try_collection_new(ty, source.entries.len())?;
        let element = ty.key();
        let mut cursor = source.entries.first();
        while let Some(index) = cursor {
            cursor = source.entries.next(index);
            let entry = source.entries.get(index);
            let value = if values && element.affine {
                source.entries.set(index, true, 0);
                entry.value
            } else {
                let value = if values { entry.value } else { entry.key };
                retain(value, element);
                value
            };
            // Reservation above guarantees push cannot allocate. No operation
            // after the first ownership transfer can fail or call user code.
            (*result).entries.push(Entry {
                key: value,
                value: 0,
            });
        }
        Ok(result as u128)
    }
}

unsafe fn try_list_slice(
    source: &mut Collection,
    start: i64,
    stop: i64,
    ty: &'static Type,
) -> Result<u128, AllocError> {
    unsafe {
        let bounds = slice_bounds(start, stop, source.entries.len());
        let result = try_collection_new(ty, bounds.len())?;
        let element = ty.key();
        // Reserve before moving anything. The source is a unique temporary for
        // affine elements; its eventual cleanup releases all unselected entries.
        for index in bounds {
            let entry = source.entries.get(index);
            let value = if element.affine {
                source.entries.set(index, false, 0);
                entry.key
            } else {
                retain(entry.key, element);
                entry.key
            };
            (*result).entries.push(Entry {
                key: value,
                value: 0,
            });
        }
        Ok(result as u128)
    }
}

/// Replace the owned children of a byte-copied value with independent copies.
/// Its inline storage must already be private. On failure it owns nothing.
unsafe fn try_copy_into(value: &mut u128, ty: &Type) -> Result<(), AllocError> {
    unsafe {
        if !ty.affine {
            return try_copy_shared(value, ty);
        }
        if let Some(record) = ty.record(*value) {
            if record.state().is_some() {
                crate::fail("cannot copy a class with custom cleanup");
            }
            for (i, (slot, ty)) in record.slots().enumerate() {
                let mut field = slot.read();
                if let Err(error) = try_copy_into(&mut field, ty) {
                    abandon_copy(record, i);
                    *value = 0;
                    return Err(error);
                }
                slot.write(field, ty);
            }
            return Ok(());
        }
        let copied = match ty.kind {
            b'B' => match ty.payload(*value) {
                Some(t) => {
                    let mut inner = ty.unpack(*value);
                    try_copy_into(&mut inner, t).map(|()| ty.pack(inner, ty.tag(*value)))
                }
                None => return Ok(()),
            },
            b'L' | b'S' | b'D' => try_copy_collection(*value, ty),
            b'O' => try_copy_box(*value as *const Boxed, ty),
            _ => return try_copy_shared(value, ty),
        };
        match copied {
            Ok(copy) => {
                *value = copy;
                Ok(())
            }
            Err(error) => {
                *value = 0;
                Err(error)
            }
        }
    }
}

/// Release the copies made of the first `copied` fields of `record`, then
/// clear every field: the rest still names the source.
unsafe fn abandon_copy(record: Record<'_>, copied: usize) {
    unsafe {
        // Walk the prefix backwards without allocating: this path handles
        // allocation failure, and records have few fields.
        for j in (0..copied).rev() {
            let (field, ty) = record.slots().nth(j).expect("copied field");
            release(field.read(), ty);
        }
        for (field, ty) in record.slots() {
            field.clear(ty);
        }
    }
}

/// Copy a value whose storage may be shared, such as text. Sharing is
/// replaced by duplication when the storage belongs to another std.actor heap
/// than the current one, so a message copied into an actor never points into
/// a heap that is discarded separately.
unsafe fn try_copy_shared(value: &mut u128, ty: &Type) -> Result<(), AllocError> {
    unsafe {
        if let Some(record) = ty.record(*value)
            && record.state().is_none()
        {
            for (i, (slot, ty)) in record.slots().enumerate() {
                let mut field = slot.read();
                if let Err(error) = try_copy_shared(&mut field, ty) {
                    abandon_copy(record, i);
                    *value = 0;
                    return Err(error);
                }
                slot.write(field, ty);
            }
            return Ok(());
        }
        if ty.kind == b'B' {
            if let Some(t) = ty.payload(*value) {
                let mut inner = ty.unpack(*value);
                try_copy_shared(&mut inner, t)?;
                *value = ty.pack(inner, ty.tag(*value));
            }
            return Ok(());
        }
        let object = *value as usize as *mut Header;
        let owned = ty.managed()
            && ty.kind != b'C'
            && !object.is_null()
            && object as usize & 1 == 0
            && crate::actor_heap::foreign(object.cast())
            && (*object).refs.load(std::sync::atomic::Ordering::Relaxed) != u64::MAX;
        if !owned {
            retain(*value, ty);
            return Ok(());
        }
        if ty.kind != b's' {
            crate::fail("std.actor: this value cannot be copied into another actor heap");
        }
        *value = strings::try_duplicate(object as *const Text)? as u128;
        Ok(())
    }
}

unsafe fn try_copy_box(source: *const Boxed, ty: &Type) -> Result<u128, AllocError> {
    unsafe {
        let content = ty.key();
        let source = std::ptr::addr_of!((*source).slot).cast::<u128>().read();
        let boxed = try_box_new(ty, source)?;
        let slot = std::ptr::addr_of_mut!((*boxed).slot).cast::<u128>();
        let mut inner = slot.read();
        let copied = try_copy_into(&mut inner, content);
        slot.write(inner);
        match copied {
            Ok(()) => Ok(boxed as u128),
            Err(error) => {
                plenty_release(boxed.cast());
                Err(error)
            }
        }
    }
}

unsafe fn try_copy_collection(value: u128, ty: &Type) -> Result<u128, AllocError> {
    unsafe {
        let source = &*(value as *const Collection);
        let result = try_collection_new(&*source.ty, source.entries.len())?;
        let owner = OwnedValue {
            value: result as u128,
            ty,
        };
        for entry in source.entries.iter() {
            // Insert byte copies that still name the source, then replace them
            // in place, so inline payloads are copied into the final buffer.
            (*result).try_store(entry.key, entry.value)?;
            if ty.kind == b'L' && ty.key().owns_nothing() {
                continue;
            }
            let row = (*result).entries.len() - 1;
            let key = (*result).entries.slot(row, false);
            let mut copy = key.read();
            if let Err(error) = try_copy_into(&mut copy, ty.key()) {
                key.write(0);
                if ty.value.is_some() {
                    (*result).entries.slot(row, true).write(0);
                }
                return Err(error);
            }
            key.write(copy);
            if let Some(t) = ty.value {
                let value = (*result).entries.slot(row, true);
                let mut copy = value.read();
                let copied = try_copy_into(&mut copy, t);
                value.write(copy);
                copied?;
            }
        }
        Ok(owner.into_value())
    }
}

/// Render `(a, b)`, or `(x=a, y=b)` with class field names.
unsafe fn render_fields(
    record: Record<'_>,
    names: Option<&[Variant]>,
    out: &mut crate::render_buffer::Buffer,
) {
    out.push(b'(');
    for (i, (slot, ty)) in record.slots().enumerate() {
        if out.failed() {
            break;
        }
        if i != 0 {
            out.extend_from_slice(b", ");
        }
        if let Some(names) = names {
            out.extend_from_slice(names[i].name.as_bytes());
            out.push(b'=');
        }
        unsafe { render(slot.read(), ty, out) };
    }
    out.push(b')');
}

unsafe fn render(value: u128, ty: &Type, out: &mut crate::render_buffer::Buffer) {
    if out.failed() {
        return;
    }
    if let Some(wrapper) = ty.wrapper {
        out.extend_from_slice(ty.name.as_bytes());
        out.push(b'(');
        out.extend_from_slice(wrapper.name.as_bytes());
        out.push(b'=');
        unsafe { render(value, wrapper.fields[0], out) };
        out.push(b')');
        return;
    }
    unsafe {
        match ty.kind {
            b'B' => {
                let tuple = ty.variants.len() == 1 && ty.name.starts_with("tuple[");
                let variant = &ty.variants[ty.tag(value)];
                // Apart from tuples, unnamed alternatives describe anonymous
                // sums, whose source-level value is just the active member.
                if !tuple && variant.name.is_empty() {
                    render(ty.unpack(value), variant.fields[0], out);
                    return;
                }
                if !tuple {
                    out.extend_from_slice(ty.name.as_bytes());
                    out.push(b'.');
                    out.extend_from_slice(variant.name.as_bytes());
                }
                if let Some(record) = ty.record(value) {
                    render_fields(record, None, out);
                } else if let Some(t) = ty.payload(value) {
                    out.push(b'(');
                    render(ty.unpack(value), t, out);
                    out.extend_from_slice(if tuple { b",)" } else { b")" });
                } else if tuple {
                    out.extend_from_slice(b"()");
                }
            }
            b'C' => {
                out.extend_from_slice(ty.name.as_bytes());
                match ty.record(value) {
                    Some(record) => render_fields(record, Some(ty.variants), out),
                    None => out.extend_from_slice(b"()"),
                }
            }
            b'O' => {
                let boxed = value as *const Boxed;
                out.extend_from_slice(b"Box(");
                render(
                    std::ptr::addr_of!((*boxed).slot).cast::<u128>().read(),
                    ty.key(),
                    out,
                );
                out.push(b')');
            }
            b'r' => {
                // A reference carried by an Option or Result: the low word
                // addresses the referenced slot and the high word is the tag
                // shift of the projected payload inside it.
                if crate::entries::scalar_bytes(ty.key()).is_some() {
                    let scalar = crate::entries::read_scalar(value as u64 as *const u8, ty.key());
                    render(scalar, ty.key(), out);
                    return;
                }
                // A class with a record may be a flat field, which has no
                // word: its value is the address after the slot.
                if ty.key().flat_field() {
                    render(value as u64 as u128 + 16, ty.key(), out);
                    return;
                }
                let slot = (value as u64 as *const u128).read();
                let shift = (value >> 64) as u32;
                let tags = (slot >> 64).checked_shr(shift).unwrap_or(0);
                render((slot & u64::MAX as u128) | (tags << 64), ty.key(), out);
            }
            b'1' => write!(out, "{}", value as i8).unwrap(),
            b'2' => write!(out, "{}", value as i16).unwrap(),
            b'3' => write!(out, "{}", value as i32).unwrap(),
            b'4' => write!(out, "{}", value as i64).unwrap(),
            b'5' => write!(out, "{}", value as u8).unwrap(),
            b'6' => write!(out, "{}", value as u16).unwrap(),
            b'7' => write!(out, "{}", value as u32).unwrap(),
            b'8' => write!(out, "{}", value as u64).unwrap(),
            b'f' => write!(out, "{:?}", f32::from_bits(value as u32)).unwrap(),
            b'd' => write!(out, "{:?}", f64::from_bits(value as u64)).unwrap(),
            b'v' => out.extend_from_slice(b"()"),
            b'c' => out.extend_from_slice(b"<function>"),
            b'H' => out.extend_from_slice(b"<closure>"),
            b'J' => out.extend_from_slice(b"<boxed callable>"),
            b'X' => out.extend_from_slice(b"<sender>"),
            b'Y' => out.extend_from_slice(b"<receiver>"),
            b'P' => out.extend_from_slice(b"<executor>"),
            b'Z' => out.extend_from_slice(b"<future>"),
            b'K' => out.extend_from_slice(b"<cancellation token>"),
            b'b' => out.extend_from_slice(if value as u8 == 0 { b"False" } else { b"True" }),
            b'A' => out.extend_from_slice(b"ReadDir"),
            b'I' => out.extend_from_slice(b"DirEntry"),
            b'M' => out.extend_from_slice(b"Metadata"),
            b'U' => byte_storage::render(value, out),
            b'Q' => write!(out, "clist(len={})", clist::len(value)).unwrap(),
            b'y' => byte_storage::render_slice(strings::bytes(&(value as *const Text)), out),
            b's' => crate::io::repr(value as *const Text, false, out),
            b'F' => out.extend_from_slice(
                if crate::files::closed(value as *const crate::files::File) {
                    b"File(closed)"
                } else {
                    b"File(open)"
                },
            ),
            b'R' => {
                let r = &*(value as *const Range);
                let signed = ty.key().kind <= b'4';
                let stop = if signed {
                    r.stop as i64 as i128
                } else {
                    r.stop as i128
                };
                write!(out, "range({}, {}, {})", r.start(signed), stop, r.step).unwrap();
            }
            _ => {
                let c = &*(value as *const Collection);
                if ty.kind == b'S' && c.entries.is_empty() {
                    out.extend_from_slice(b"set()");
                    return;
                }
                out.push(if ty.kind == b'L' { b'[' } else { b'{' });
                for (i, entry) in c.entries.iter().enumerate() {
                    if out.failed() {
                        break;
                    }
                    if i != 0 {
                        out.extend_from_slice(b", ");
                    }
                    render(entry.key, ty.key(), out);
                    if ty.kind == b'D' {
                        out.extend_from_slice(b": ");
                        render(entry.value, ty.value(), out);
                    }
                }
                out.push(if ty.kind == b'L' { b']' } else { b'}' });
            }
        }
    }
}

/// Narrow ABI: a live borrowed list owner, with no ownership transfer. Besides
/// these helpers, generated code reads only the dense row fields named in
/// `list_layout`, to index scalar lists inline.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_list_len(owner: *const u8) -> u64 {
    // SAFETY: the compiler keeps the list's owner alive through the call.
    unsafe { (*(owner.cast::<Collection>())).entries.len() as u64 }
}

/// Narrow ABI: the address of a live borrowed list's first row. Borrowed
/// iteration addresses row `i` at `rows + i * row_bytes`, exactly the element
/// slot a generic element reference names. The compiler holds the list's loan
/// for the whole loop, so no mutation can move the rows while it uses them.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_list_rows(owner: *mut u8) -> *mut u128 {
    // SAFETY: the compiler keeps the list's owner alive through the call.
    unsafe { (*(owner.cast::<Collection>())).entries.rows() }
}

/// Narrow ABI: the distance in bytes between a live list's rows.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_list_row_bytes(owner: *const u8) -> u64 {
    // SAFETY: the compiler keeps the list's owner alive through the call.
    unsafe { (*(owner.cast::<Collection>())).entries.row_bytes() as u64 }
}

/// Narrow ABI: the total sort order of two values of `descriptor`'s type as
/// -1, 0, or 1. Operands arrive as 64-bit halves; neither changes ownership.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_order_compare(
    a_low: u64,
    a_high: u64,
    b_low: u64,
    b_high: u64,
    descriptor: *const Type,
) -> i64 {
    let a = u128::from(a_high) << 64 | u128::from(a_low);
    let b = u128::from(b_high) << 64 | u128::from(b_low);
    // SAFETY: the compiler passes two live values of the orderable descriptor.
    unsafe { compare(a, b, &*descriptor) as i64 }
}

/// A comparator generated for one key sort: callback reference and two row
/// value slots, returning -1, 0, or 1.
type KeyOrder = unsafe extern "C" fn(*const u128, *const u128, *const u128) -> i64;

/// Narrow ABI: sort a live, exclusively borrowed list by a generated key
/// comparator. Rows move inside the runtime; the comparator only observes
/// them, so no element is retained, released, or copied, and nothing is
/// allocated. The comparator calls user code, which cannot reach the list
/// while the caller holds its exclusive loan.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_list_sort_by(
    owner: *mut u8,
    compare: KeyOrder,
    environment: *const u128,
    descending: i64,
) {
    // SAFETY: the compiler passes a live dense list it exclusively borrows, its
    // matching adapter, and the adapter's callback reference.
    unsafe {
        let entries = &mut (*owner.cast::<Collection>()).entries;
        entries.sort_unstable_rows_by(|a, b| {
            let order = {
                let _caller = crate::CallerScope::save();
                compare(environment, &a, &b).cmp(&0)
            };
            if descending != 0 {
                order.reverse()
            } else {
                order
            }
        });
    }
}

/// Returns the low 64 payload bits of an integer, float, or boolean list row.
/// No pointer into the buffer escapes, including across later growing mutations.
/// Generated code reads a borrowed list's rows inline (issue #509); this
/// serves owned temporaries and iteration.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_list_scalar_get(
    owner: *const u8,
    i: i64,
    site: *const crate::Site,
) -> u64 {
    // SAFETY: generated callers supply a live list of scalars. Bounds and
    // negative indices use the same normalization as the generic dispatcher.
    unsafe {
        let collection = &*owner.cast::<Collection>();
        let i = index(i, collection.entries.len(), site);
        collection.entries.get(i).key as u64
    }
}

/// Reports list index `index`, as written, out of bounds for `len` rows.
/// Generated code calls it from inline scalar row reads and writes, which
/// normalize and check the index as `index` does (issue #509).
#[unsafe(no_mangle)]
extern "C" fn plenty_trap_list_index(index: i64, len: u64, site: *const crate::Site) -> ! {
    out_of_bounds(index, len as usize, site)
}

/// Narrow ABI: the address of row `i` of a live borrowed list, the slot a
/// generic element reference (opcode 112) names. Bounds and negative indices
/// use the same normalization, site, and message as the generic dispatcher.
/// The compiler holds the list's loan while it uses the address.
/// Scalar rows require a load/store of the declared width; the returned address
/// need not have u128 alignment or space for a canonical slot.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_list_row(
    owner: *mut u8,
    i: i64,
    site: *const crate::Site,
) -> *mut u128 {
    // SAFETY: generated callers supply a live list owner under a loan.
    unsafe {
        let collection = &mut *owner.cast::<Collection>();
        let i = index(i, collection.entries.len(), site);
        collection.entries.slot(i, false)
    }
}

/// Exports `plenty_op_<N>` for each runtime operation `N` with
/// [`operation`]'s argument layout. Generated code calls the operation's own
/// symbol, and `operation` and the module handlers it reaches are inlined
/// with a constant `N`, so each entry holds only its operation's code and
/// `--gc-sections` drops every operation a program never calls, with any std
/// module that only those operations reach (issue #501).
macro_rules! operation_entries {
    ($($op:literal)*) => {$(
        const _: () = {
            #[unsafe(export_name = concat!("plenty_op_", $op))]
            unsafe extern "C" fn entry(
                args: *const u128,
                descriptor: *const Type,
                site: *const crate::Site,
            ) {
                // SAFETY: as for `operation`.
                unsafe { operation($op, args, descriptor, site) }
            }
        };
    )*};
}

// Lists, dictionaries, sets, ranges, text, boxes, generators and printing.
operation_entries! {
    1 4 5 6 7 8 10 12 13 15 24 26 27 28 29 32 33 34 35 36 37 38 39 40 41 42 43 44 45
    46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72
    73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99
    100 101 102 103 104 105 106 107 110 111 112 113 114 117 118 119 120 121 122 123 124
    125 126 127 128 129 130 131 132 133 134 135 136 140 166 167 366 510 511 580 581 582
    583 584 585
}
// `std.bytes` storage and single-call byte transport.
operation_entries! {
    142 143 144 145 146 147 148 149 170 171 172 173 174 175 176 177 178 179 180 181
    182 183 188 189 190 191 230 231 232 233 234 235 236 237 238 239 248 280 281 282
}
// `std.fs` paths and descriptors, file seeking and file readiness.
operation_entries! {
    150 151 152 153 154 155 156 157 158 159 160 161 162 163 164 192 260 261 262 263
    264 265 266 267 268 269 270 271 272 273 390 512 513 514 515
}
// `std.time`.
operation_entries! {
    184 185 186 187
}
// `std.process` and `std.signal`.
operation_entries! {
    193 194 195 196 197 198 199 228 229 500
}
// `std.unicode`.
operation_entries! {
    224 225 226 227
}
// `std.crypto`.
operation_entries! {
    300 301 302 303 304 305 306 307
}
// Advanced `std.crypto` primitives.
operation_entries! {
    308 309 310 311 312 313 314 315 316 317 318
}
// `std.crypto` RSA and P-256 keys.
operation_entries! {
    340 341 342 343 344 345 346 347 348
}
// `std.compress`.
operation_entries! {
    320 321 322 323 324 325
}
// `std.regex`.
operation_entries! {
    360 361 362 363 364 365
}
// `std.net`.
operation_entries! {
    410 411 412 413 414 415 416 417 418
}
// `std.ready`.
operation_entries! {
    419 420 421 422 423
}
// `std.tls`.
operation_entries! {
    470 471 472 473 474 475 476 477 478 479 480 481 482 483 484 485 486 487 488 489
    490 491 492 493 494
}
// Primitive column storage.
operation_entries! {
    521 522 523 524 525 526 527 528 529 530 531
}
// `std.db.sqlite`, present only in the application runtime.
#[cfg(plenty_runtime_sqlite)]
operation_entries! {
    440 441 442 443 444 445 446 447 448 449 450 451 452 453 454 455 456 457 458 459
    460 461 462 463 464 465 466 467 468 469
}

/// Read only the words the compiler initializes for this opcode. The fixed
/// inputs are always initialized (unused ones are zero); scratch operands
/// after the result are initialized only when the shared ABI declares them.
#[inline(always)]
unsafe fn read_operands<const N: usize>(op: i64, args: *const u128) -> [u128; N] {
    let extra = scratch_operand_count(op);
    assert!(
        N >= 3 + extra && N <= 5,
        "runtime operand array is too small"
    );
    let mut operands = [0; N];
    // SAFETY: the caller supplies the three fixed words and every declared
    // scratch input. Never form a reference to any uninitialized spare word.
    unsafe {
        for (index, operand) in operands[..3].iter_mut().enumerate() {
            *operand = *args.add(index);
        }
        for index in 0..extra {
            operands[3 + index] = *args.add(4 + index);
        }
    }
    operands
}

/// Runs operation `op`. Only the `plenty_op_<N>` entries call it, each with
/// its own constant `op`.
#[inline(always)]
unsafe fn operation(op: i64, args: *const u128, descriptor: *const Type, site: *const crate::Site) {
    // SAFETY: generated code supplies three aligned inputs and an output slot
    // followed by 32 bytes of scratch range storage. `site` is null for
    // operations that cannot trap.
    unsafe {
        let out = args.add(3).cast_mut();
        if matches!(op, 300..350 | 440..500) {
            let operands = read_operands::<4>(op, args);
            out.write(match op {
                300..308 => crypto::call(op, operands),
                308..320 => crypto_advanced::call(op, operands),
                320..340 => compress::call(op, operands),
                340..350 => crypto_keys::call(op, operands),
                #[cfg(plenty_runtime_sqlite)]
                440..470 => sqlite::call(op, operands),
                470..500 => tls::call(op, operands),
                _ => crate::fail("invalid runtime operation"),
            });
            return;
        }
        if op == 10 {
            let range = out.add(1).cast::<Range>();
            range.write(Range::new_at(
                *args as u64,
                *args.add(1) as u64,
                *args.add(2) as i64,
                (*descriptor).key().kind <= b'4',
                site,
            ));
            out.write(range as u128);
            return;
        }
        if (521..=531).contains(&op) {
            let operands = read_operands::<4>(op, args);
            out.write(clist::call(op, operands, &*descriptor, site));
            return;
        }
        if matches!(op, 170..=183 | 188..=191 | 230..=239 | 248 | 280..=282) {
            let operands = read_operands::<5>(op, args);
            out.write(byte_storage::call(op, operands, descriptor, site));
            return;
        }
        if (419..424).contains(&op) {
            let operands = read_operands::<5>(op, args);
            out.write(readiness::call(op, operands, descriptor));
            return;
        }
        if (410..440).contains(&op) {
            let operands = read_operands::<5>(op, args);
            out.write(net::call(op, operands, descriptor));
            return;
        }
        if op == 24 {
            out.write(0);
            let ready = plenty_generator_resume(*args as *mut Generator, out);
            out.write(wrap(out.read(), ready as u64));
            return;
        }
        out.write(collection_at(
            op,
            *args,
            *args.add(1),
            *args.add(2),
            descriptor,
            site,
        ));
    }
}

#[cfg(test)]
pub(crate) unsafe fn collection(
    op: i64,
    a: u128,
    b: u128,
    value: u128,
    descriptor: *const Type,
) -> u128 {
    unsafe { collection_at(op, a, b, value, descriptor, std::ptr::null()) }
}

/// `site` locates the operation in a trap; null prints the message alone.
// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(crate) unsafe fn collection_at(
    op: i64,
    a: u128,
    b: u128,
    value: u128,
    descriptor: *const Type,
    site: *const crate::Site,
) -> u128 {
    // SAFETY: the independent compiler checker supplies each opcode's declared
    // types and arity. No general Rust reference is returned to generated code.
    unsafe {
        if !descriptor.is_null() && (*descriptor).kind == b'R' {
            let signed = (*descriptor).key().kind <= b'4';
            match op {
                4 | 6 => {
                    let range = &*(a as *const Range);
                    return range.at(index(b as i64, range.len as usize, site), signed);
                }
                5 => return (*(a as *const Range)).len as u128,
                7 => return (*(b as *const Range)).contains(a, signed) as u128,
                _ => {}
            }
        }
        if op == 33 {
            return match try_copy(a, &*descriptor, b as *mut u128) {
                Ok(value) => wrap(value, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            };
        }
        // A `bytes` literal: immortal read-only data in the text layout,
        // indexed by byte with the list index rules.
        if !descriptor.is_null() && (*descriptor).kind == b'y' && matches!(op, 4..=7) {
            return match op {
                5 => strings::byte_len(a as *const Text) as u128,
                7 => strings::bytes(&(b as *const Text)).contains(&(a as u8)) as u128,
                _ => {
                    let text = a as *const Text;
                    let data = strings::bytes(&text);
                    data[index(b as i64, data.len(), site)] as u128
                }
            };
        }
        // A newtype over str takes the generic paths: a class export adapter
        // relocates it with op 117, which is not a string operation.
        if !descriptor.is_null()
            && (*descriptor).kind == b's'
            && (*descriptor).wrapper.is_none()
            && !matches!(op, 35 | 110 | 111 | 122)
        {
            let text = a as *const Text;
            return match op {
                5 => strings::scalar_len(text) as u128,
                12 => strings::byte_len(text) as u128,
                13 => strings::at_byte(text, b as usize) as u128,
                4 | 6 => strings::at(text, b as i64, site) as u128,
                7 => strings::plenty_contains(b as *const Text, text) as u128,
                _ => crate::fail("invalid string operation"),
            };
        }
        match op {
            150..=164 => filesystem::call(op, a, b, descriptor),
            260..=273 => filesystem::rooted_call(op, a, b, value, descriptor),
            512..=514 => watch::call(op, a, b, descriptor),
            184..=187 => crate::time::call(op, a),
            193..=199 | 228..=229 | 500 => process::call(op, a, b, value, descriptor),
            224..=227 => crate::unicode::call(op, a, b, value),
            390 => crate::text_io::result(crate::files::wait_ready(
                a as *const crate::files::File,
                b as u8,
                value as u64,
            )),
            360..=365 => crate::regex::call(op, a, b, value),
            366 => strings::ascii_equal_folded(a as *const Text, b as *const Text) as u128,
            192 => crate::text_io::result(crate::files::seek_bytes(
                a as *mut crate::files::File,
                b as u8,
                value as i64,
            )),
            112 => {
                let owner = *(a as *const u128);
                let c = &mut *(owner as *mut Collection);
                let position = if c.ty().kind == b'L' {
                    index(b as i64, c.entries.len(), site)
                } else {
                    c.find(b).unwrap_or_else(|| {
                        crate::fail_kind_at(
                            crate::actor_heap::TrapKind::Index,
                            site,
                            format_args!("dictionary key not found"),
                        )
                    })
                };
                c.entries.slot(position, c.ty().kind != b'L') as u128
            }
            // `dict.get_mut`: the value slot's address, or Nothing; never traps.
            113 => {
                let owner = *(a as *const u128);
                let c = &mut *(owner as *mut Collection);
                match c.find(b) {
                    Some(position) => wrap(c.entries.slot(position, true) as u128, 1),
                    None => wrap(0, 0),
                }
            }
            114 => strings::at(a as *const Text, b as i64, site) as u128,
            167 => {
                let collection = &*(a as *const Collection);
                if collection.len() != b as usize {
                    crate::fail_at(
                        site,
                        format_args!(
                            "unpacking requires exactly {} list elements; got {}",
                            b,
                            collection.len()
                        ),
                    );
                }
                plenty_retain(a as *mut Header);
                a
            }
            110 | 111 | 166 => {
                #[cfg(feature = "allocation-checks")]
                if op == 111
                    && (*descriptor).kind == b's'
                    && strings::bytes(&(a as *const Text)).starts_with(b"__test_")
                {
                    crate::io::plenty_println(a as *const Text);
                    return wrap(0, 0);
                }
                if op == 111 && (*descriptor).kind == b's' && (*descriptor).wrapper.is_none() {
                    // A string already has its final byte representation. Printing
                    // it needs no formatting buffer, including inside destructors. A
                    // newtype over str formats as the class instead.
                    let mut stdout = std::io::stdout().lock();
                    return crate::text_io::result(
                        stdout
                            .write_all(strings::bytes(&(a as *const Text)))
                            .and_then(|()| stdout.write_all(b"\n"))
                            .map(|()| 0)
                            .map_err(crate::text_io::Error::from),
                    );
                }
                let mut out = crate::render_buffer::Buffer::default();
                if op == 166 {
                    let ty = &*descriptor;
                    let mut item = |i: usize, value: u128, field: &Type| {
                        if i != 0 {
                            out.push(b' ');
                        }
                        if field.kind == b's' && field.wrapper.is_none() {
                            out.extend_from_slice(strings::bytes(&(value as *const Text)));
                        } else {
                            render(value, field, &mut out);
                        }
                    };
                    for (i, field) in ty.variants[0].fields.iter().enumerate() {
                        item(i, (a as *const u128).add(i).read(), field);
                    }
                } else {
                    render(a, &*descriptor, &mut out);
                }
                if op == 111 || op == 166 {
                    out.push(b'\n');
                    let result =
                        out.finish()
                            .map_err(crate::text_io::Error::from)
                            .and_then(|bytes| {
                                std::io::stdout()
                                    .lock()
                                    .write_all(&bytes)
                                    .map(|()| 0)
                                    .map_err(crate::text_io::Error::from)
                            });
                    crate::text_io::result(result)
                } else {
                    match out.finish().and_then(|bytes| {
                        strings::try_new(std::str::from_utf8(&bytes).expect("renderer emits UTF-8"))
                    }) {
                        Ok(text) => wrap(text as u128, 0),
                        Err(error) => wrap(wrap(0, error as u64), 1),
                    }
                }
            }
            515 => crate::text_io::result(filesystem::contiguous(a).and_then(|path| {
                crate::files::open_bytes(
                    &path,
                    strings::utf8(&(b as *const Text)),
                    (*descriptor).variants[0].fields[0],
                )
            })),
            89 => crate::text_io::result(crate::files::open(
                strings::utf8(&(a as *const Text)),
                strings::utf8(&(b as *const Text)),
                (*descriptor).variants[0].fields[0],
            )),
            90 => crate::text_io::result(crate::files::close(a as *mut crate::files::File)),
            91 => crate::files::closed(a as *const crate::files::File) as u128,
            99 | 100 => crate::text_io::result(crate::files::capability(
                a as *const crate::files::File,
                op == 100,
            )),
            92 => crate::text_io::result(crate::files::read(a as *mut crate::files::File)),
            101 => crate::text_io::result(crate::files::tell(a as *mut crate::files::File)),
            106 => crate::text_io::result(write_file_lines(
                a as *mut crate::files::File,
                b as *const Collection,
            )),
            105 => crate::text_io::result(crate::files::readlines(
                a as *mut crate::files::File,
                (*descriptor).variants[0].fields[0],
            )),
            103 | 104 => crate::text_io::result(crate::files::truncate(
                a as *mut crate::files::File,
                (op == 104).then_some(b as i64),
            )),
            102 => {
                crate::text_io::result(crate::files::seek(a as *mut crate::files::File, b as u64))
            }
            96 => crate::text_io::result(crate::files::readline(a as *mut crate::files::File)),
            97 | 98 => crate::text_io::result(crate::files::read_sized(
                a as *mut crate::files::File,
                b as i64,
                op == 98,
            )),
            93 => crate::text_io::result(crate::files::write(
                a as *mut crate::files::File,
                strings::utf8(&(b as *const Text)),
            )),
            94 | 95 => {
                crate::text_io::result(crate::files::flush(a as *mut crate::files::File, op == 95))
            }
            87 | 88 => crate::text_io::result(crate::text_io::write_text(
                strings::utf8(&(a as *const Text)),
                strings::utf8(&(b as *const Text)),
                op == 88,
            )),
            86 => crate::text_io::result(crate::text_io::read_text(strings::utf8(
                &(a as *const Text),
            ))),
            85 => crate::text_io::result(crate::text_io::arguments(
                (*descriptor).variants[0].fields[0],
            )),
            84 => crate::text_io::input(),
            142..=149 => crate::text_io::result(bytes::call(op, a, b, descriptor)),
            80 | 82 => crate::io::write_stream(a as *const Text, op == 82),
            510 => crate::io::report_write(a as *const Text, b as i64),
            511 => crate::io::is_terminal(a as u8) as u128,
            81 | 83 => crate::io::flush_stream(op == 83),
            79 => match crate::numbers::format(a, (*descriptor).kind) {
                Ok(text) => wrap(text as u128, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            78 => crate::numbers::parse(strings::utf8(&(a as *const Text)), (*descriptor).kind),
            76 | 77 => {
                let a_text = a as *const Text;
                let source = strings::utf8(&a_text);
                if op == 76 {
                    source.is_ascii() as u128
                } else {
                    (!source.is_empty() && source.chars().all(char::is_whitespace)) as u128
                }
            }
            73..=75 => {
                let list = &*(a as *const Collection);
                let matches = |entry: &Entry| equal(entry.key, b, list.ty().key());
                if op == 73 {
                    list.entries.iter().filter(|entry| matches(entry)).count() as u128
                } else {
                    let index = if op == 74 {
                        list.entries.iter().position(|entry| matches(&entry))
                    } else {
                        list.entries.iter().rposition(|entry| matches(&entry))
                    };
                    match index {
                        Some(index) => wrap(index as u128, 1),
                        None => wrap(0, 0),
                    }
                }
            }
            71 | 72 => {
                let a_text = a as *const Text;
                let source = strings::utf8(&a_text);
                let b_text = b as *const Text;
                let affix = strings::utf8(&b_text);
                let trimmed = if op == 71 {
                    source.strip_prefix(affix)
                } else {
                    source.strip_suffix(affix)
                }
                .unwrap_or(source);
                match strings::try_new(trimmed) {
                    Ok(text) => wrap(text as u128, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            69 | 70 => {
                let target = &mut *(a as *mut Collection);
                let other = &*(b as *const Collection);
                let ty = &*target.ty;
                target.entries.retain(|entry| {
                    if other.find(entry.key).is_some() == (op == 69) {
                        true
                    } else {
                        release(entry.key, ty.key());
                        false
                    }
                });
                target.table.fill(0);
                for index in target.entries.indices() {
                    let entry = target.entries.get(index);
                    let bucket = target.bucket(entry.key);
                    target.table[bucket] = index + 1;
                }
                plenty_retain(a as *mut Header);
                a
            }
            SET_UNION..=SET_SYMMETRIC_DIFFERENCE
            | KEY_VIEW_UNION..=KEY_VIEW_SYMMETRIC_DIFFERENCE => {
                let (left, right) = (&*(a as *const Collection), &*(b as *const Collection));
                let (op, result_type) = match op {
                    KEY_VIEW_UNION => (SET_UNION, &*descriptor),
                    KEY_VIEW_INTERSECTION => (SET_INTERSECTION, &*descriptor),
                    KEY_VIEW_DIFFERENCE => (SET_DIFFERENCE, &*descriptor),
                    KEY_VIEW_SYMMETRIC_DIFFERENCE => (SET_SYMMETRIC_DIFFERENCE, &*descriptor),
                    _ => (op, &*left.ty),
                };
                let left_entries = left.entries.iter().filter(|entry| match op {
                    SET_UNION => true,
                    SET_INTERSECTION => right.find(entry.key).is_some(),
                    _ => right.find(entry.key).is_none(),
                });
                let right_entries = right.entries.iter().filter(|entry| {
                    matches!(op, SET_UNION | SET_SYMMETRIC_DIFFERENCE)
                        && left.find(entry.key).is_none()
                });
                let entries = left_entries.chain(right_entries);
                match try_set_from_entries(entries, result_type) {
                    Ok(set) => wrap(set, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            KEY_VIEW_TO_SET => {
                let source = &*(a as *const Collection);
                match try_set_from_entries(source.entries.iter(), &*descriptor) {
                    Ok(set) => wrap(set, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            SET_IS_SUBSET..=SET_IS_DISJOINT | KEY_VIEW_EQUAL => {
                let (left, right) = (&*(a as *const Collection), &*(b as *const Collection));
                let result = match op {
                    KEY_VIEW_EQUAL => {
                        left.entries.len() == right.entries.len()
                            && left
                                .entries
                                .iter()
                                .all(|entry| right.find(entry.key).is_some())
                    }
                    SET_IS_SUBSET => {
                        left.entries.len() <= right.entries.len()
                            && left
                                .entries
                                .iter()
                                .all(|entry| right.find(entry.key).is_some())
                    }
                    SET_IS_SUPERSET => {
                        right.entries.len() <= left.entries.len()
                            && right
                                .entries
                                .iter()
                                .all(|entry| left.find(entry.key).is_some())
                    }
                    _ => {
                        let (small, large) = if left.entries.len() <= right.entries.len() {
                            (left, right)
                        } else {
                            (right, left)
                        };
                        small
                            .entries
                            .iter()
                            .all(|entry| large.find(entry.key).is_none())
                    }
                };
                result as u128
            }
            60 | 61 => match (*(a as *mut Collection)).try_update(&mut *(b as *mut Collection)) {
                Ok(()) => wrap(0, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            59 => match (*(a as *mut Collection)).try_extend(&mut *(b as *mut Collection)) {
                Ok(()) => wrap(0, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            58 => {
                let c = &mut *(a as *mut Collection);
                let ty = &*c.ty;
                c.table.fill(0);
                // Drain preserves allocated capacity. Clear the visible length
                // before invoking cleanup, then destroy entries in source order.
                for entry in c.entries.drain() {
                    release(entry.key, ty.key());
                    if let Some(value) = ty.value {
                        release(entry.value, value);
                    }
                }
                plenty_retain(a as *mut Header);
                a
            }
            126 => {
                let c = &mut *(a as *mut Collection);
                let len = c.entries.len();
                c.entries
                    .swap(index(b as i64, len, site), index(value as i64, len, site));
                plenty_retain(a as *mut Header);
                a
            }
            127 => {
                let c = &mut *(a as *mut Collection);
                let element = (*c.ty).key();
                let descending = b != 0;
                let direct = |order: Ordering| if descending { order.reverse() } else { order };
                match FlatTuple::new(element) {
                    Some(tuple) => c
                        .entries
                        .sort_unstable_by(|x, y| direct(tuple.compare(x, y, element))),
                    None => c
                        .entries
                        .sort_unstable_by(|x, y| direct(compare(x, y, element))),
                }
                plenty_retain(a as *mut Header);
                a
            }
            57 => {
                (*(a as *mut Collection)).entries.reverse();
                plenty_retain(a as *mut Header);
                a
            }
            56 => match strings::try_repeat(a as *const Text, b as i64) {
                Ok(text) => wrap(text as u128, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            53..=55 => match strings::try_strip(a as *const Text, op != 55, op != 54) {
                Ok(text) => wrap(text as u128, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            52 => strings::plenty_text_count(a as *const Text, b as *const Text) as u128,
            50 | 51 => {
                let (a, b) = (a as *const Text, b as *const Text);
                let found = if op == 50 {
                    strings::plenty_text_find(a, b)
                } else {
                    strings::plenty_text_rfind(a, b)
                };
                match found {
                    u64::MAX => wrap(0, 0),
                    index => wrap(index as u128, 1),
                }
            }
            48 => strings::plenty_text_starts_with(a as *const Text, b as *const Text) as u128,
            49 => strings::plenty_text_ends_with(a as *const Text, b as *const Text) as u128,
            47 => {
                match strings::try_replace(a as *const Text, b as *const Text, value as *const Text)
                {
                    Ok(text) => wrap(text as u128, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            46 => match strings::try_slice(a as *const Text, b as i64, value as i64) {
                Ok(text) => wrap(text as u128, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            45 => match try_list_slice(
                &mut *(a as *mut Collection),
                b as i64,
                value as i64,
                &*descriptor,
            ) {
                Ok(list) => wrap(list, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            43 | 44 => {
                // Result[list[element], AllocError] carries the snapshot layout.
                let ty = (*descriptor).variants[0].fields[0];
                match try_dictionary_snapshot(&mut *(a as *mut Collection), ty, op == 44) {
                    Ok(list) => wrap(list, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            37 => match strings::scalar(a as *const Text, b as i64) {
                Some(text) => wrap(text as u128, 1),
                None => wrap(0, 0),
            },
            107 => {
                let ty = (*descriptor).variants[0].fields[0];
                let a_text = a as *const Text;
                let pieces = crate::text_lines::Lines::new(strings::utf8(&a_text), b != 0);
                match try_text_pieces(pieces, ty) {
                    Ok(list) => wrap(list, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            129 => {
                let ty = (*descriptor).variants[0].fields[0];
                let text = a as *const Text;
                match try_text_pieces(strings::utf8(&text).split_whitespace(), ty) {
                    Ok(list) => wrap(list, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            130 => {
                let text = a as *const Text;
                let mut characters = strings::utf8(&text).chars();
                match (characters.next(), characters.next()) {
                    (Some(character), None) => wrap(character as u128, 0),
                    _ => wrap(wrap(0, 0), 1),
                }
            }
            131 => match strings::from_codepoint(a as u32) {
                Some(text) => wrap(text as u128, 0),
                None => wrap(wrap(0, 1), 1),
            },
            132 => {
                let a = a as *const Text;
                let b = b as *const Text;
                match strings::utf8(&a).cmp(strings::utf8(&b)) {
                    std::cmp::Ordering::Less => (-1i64) as u128,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                }
            }
            133 | 134 => {
                let text = a as *const Text;
                let source = strings::utf8(&text);
                (!source.is_empty()
                    && source.bytes().all(|byte| {
                        if op == 133 {
                            byte.is_ascii_digit()
                        } else {
                            byte.is_ascii_alphabetic()
                        }
                    })) as u128
            }
            135 | 136 => match strings::try_case(a as *const Text, op == 136) {
                Ok(text) => wrap(text as u128, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            36 => {
                // The compiler supplies Result[list[str], AllocError] metadata.
                let ty = (*descriptor).variants[0].fields[0];
                match try_split(a as *const Text, b as *const Text, ty, site) {
                    Ok(list) => wrap(list, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            34 | 35 => {
                let result = if op == 34 {
                    strings::try_concat(a as *const Text, b as *const Text)
                } else {
                    // The compiler supplies the iterable's metadata.
                    let separator = a as *const Text;
                    match (*descriptor).kind {
                        b's' => strings::try_join_scalars(separator, b as *const Text),
                        b'G' => strings::try_join_generator(separator, b as *mut Generator),
                        _ => {
                            let source = &*(b as *const Collection);
                            strings::try_join(
                                Some(separator),
                                source.entries.iter().map(|entry| entry.key as *const Text),
                            )
                        }
                    }
                };
                match result {
                    Ok(text) => wrap(text as u128, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            32 => {
                let result = if (a as i64) < 0 {
                    Err(AllocError::CapacityOverflow)
                } else {
                    try_collection_new(&*descriptor, a as usize)
                };
                match result {
                    Ok(collection) => wrap(collection as u128, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            26 => {
                retain(a, &*descriptor);
                0
            }
            27 => {
                release(a, &*descriptor);
                0
            }
            28 | 29 => {
                let c = &mut *(a as *mut Collection);
                let result = if op == 28 {
                    if (b as i64) < 0 {
                        Err(AllocError::CapacityOverflow)
                    } else {
                        c.try_reserve(b as usize)
                    }
                } else {
                    c.try_insert(b, value)
                };
                match result {
                    Ok(()) => wrap(0, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            1 => {
                (*(a as *mut Collection)).insert(b, value);
                plenty_retain(a as *mut Header);
                a
            }
            42 => {
                let c = &*(a as *const Collection);
                match checked_index(b as i64, c.entries.len()) {
                    Some(index) => {
                        let value = c.entries.get(index).key;
                        retain(value, c.ty().key());
                        wrap(value, 1)
                    }
                    None => wrap(0, 0),
                }
            }
            40 => {
                let c = &mut *(a as *mut Collection);
                match checked_index(b as i64, c.entries.len()) {
                    Some(index) => wrap(c.entries.remove(index).key, 1),
                    None => wrap(0, 0),
                }
            }
            41 => (*(a as *mut Collection)).remove(b).is_some() as u128,
            39 => match (*(a as *mut Collection)).remove(b) {
                Some(value) => wrap(value, 1),
                None => wrap(0, 0),
            },
            38 => {
                let c = &*(a as *const Collection);
                match c.find(b) {
                    Some(i) => {
                        let value = c.entries.get(i).value;
                        retain(value, c.ty().value());
                        wrap(value, 1)
                    }
                    None => wrap(0, 0),
                }
            }
            123 => (*(a as *const Collection))
                .entries
                .first()
                .map_or(0, |i| i as u128 + 1),
            124 => (*(a as *const Collection))
                .entries
                .next(b as usize - 1)
                .map_or(0, |i| i as u128 + 1),
            125 => {
                let c = &*(a as *const Collection);
                let key = c.entries.get(b as usize - 1).key;
                retain(key, c.ty().key());
                key
            }
            140 => {
                let owner = *(a as *const u128);
                let c = &mut *(owner as *mut Collection);
                c.entries.slot(b as usize - 1, false) as u128
            }
            4 | 6 => {
                let c = &*(a as *const Collection);
                if op == 4 && c.ty().kind == b'D' {
                    let i = c.find(b).unwrap_or_else(|| {
                        crate::fail_kind_at(
                            crate::actor_heap::TrapKind::Index,
                            site,
                            format_args!("dictionary key not found"),
                        )
                    });
                    retain(c.entries.get(i).value, c.ty().value());
                    c.entries.get(i).value
                } else {
                    c.at(index(b as i64, c.len(), site))
                }
            }
            5 => (*(a as *const Collection)).len() as u128,
            15 => {
                let c = &mut *(a as *mut Collection);
                if c.ty().kind != b'L' {
                    crate::fail("owned iteration requires a list");
                }
                let i = index(b as i64, c.len(), site);
                {
                    let value = c.entries.get(i).key;
                    c.entries.set(i, false, 0);
                    value
                }
            }
            7 => {
                let c = &*(b as *const Collection);
                if c.ty().kind != b'L' {
                    c.find(a).is_some() as u128
                } else {
                    c.entries
                        .iter()
                        .any(|entry| equal(a, entry.key, c.ty().key())) as u128
                }
            }
            8 => equal(
                a,
                b,
                if descriptor.is_null() {
                    value_type(a)
                } else {
                    &*descriptor
                },
            ) as u128,
            117 => ranges::copy_payload(a, &*descriptor, b as *mut Range),
            118 => match strings::try_c_string(a as *const Text) {
                Ok(text) => wrap(text as u128, 0),
                Err(strings::CStrError::EmbeddedNul) => wrap(wrap(0, 0), 1),
                Err(strings::CStrError::Allocation(error)) => {
                    wrap(wrap(wrap(0, error as u64), 1), 1)
                }
            },
            119 => match try_box_new(&*descriptor, a) {
                Ok(boxed) => wrap(boxed as u128, 0),
                Err(error) => {
                    // A failed box drops the value it would have owned.
                    release(a, (*descriptor).key());
                    wrap(wrap(0, error as u64), 1)
                }
            },
            // A callable box retains its concrete content descriptor for drops.
            // The generated shared-call adapter occupies the box's padding word.
            128 => match try_box_new(&*descriptor, a) {
                Ok(boxed) => {
                    (*boxed).padding = b as u64;
                    wrap(boxed as u128, 0)
                }
                Err(error) => {
                    release(a, (*descriptor).key());
                    wrap(wrap(0, error as u64), 1)
                }
            },
            120 => {
                box_take(a as *mut Boxed);
                0
            }
            // An empty box for content the caller stores afterwards.
            121 => match try_box_new(&*descriptor, 0) {
                Ok(boxed) => wrap(boxed as u128, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            // The value is the process status of a `main` that returned `Err`.
            122 => {
                entry_error::report(a, descriptor);
                1
            }
            24 => {
                let mut item = 0;
                let ready = plenty_generator_resume(a as *mut Generator, &mut item);
                wrap(item, ready as u64)
            }
            _ => crate::fail("invalid collection operation"),
        }
    }
}

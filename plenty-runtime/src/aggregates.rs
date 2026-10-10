//! Rust-owned metadata and buffers behind a small, layout-stable native prefix.
use crate::entries::{Entries, Entry};
use crate::generators::{plenty_generator_resume, Generator};
use crate::memory::{self, plenty_release, plenty_retain, AllocError, Header};
use crate::ranges::{self, Range};
use crate::strings::{self, Text};
use std::io::Write;

mod entry_error;
pub(crate) mod executor_map;
pub(crate) mod executor_reduce;
#[cfg(test)]
mod removal_tests;

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
}
const _: () = {
    assert!(size_of::<Type>() == 64);
    assert!(std::mem::offset_of!(Type, key) == 8);
    assert!(std::mem::offset_of!(Type, name) == 24);
    assert!(std::mem::offset_of!(Type, variants) == 40);
    assert!(std::mem::offset_of!(Type, drop) == 56);
    assert!(size_of::<Variant>() == 32);
};
impl Type {
    fn managed(&self) -> bool {
        self.kind == b'B'
            || matches!(
                self.kind,
                b's' | b'L'
                    | b'S'
                    | b'D'
                    | b'E'
                    | b'C'
                    | b'G'
                    | b'F'
                    | b'X'
                    | b'Y'
                    | b'P'
                    | b'Z'
                    | b'K'
                    | b'O'
            )
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
const _: () = assert!(std::mem::offset_of!(Collection, ty) == 16);

#[derive(Clone, Copy)]
pub(crate) enum Fields<'a> {
    /// Class descriptors name each field as a one-field variant.
    Class(&'a [Variant]),
    Variant(&'a [&'static Type]),
}
/// Typed field slots inside their owner's storage. Each slot reserves its
/// type's complete inline payload. A class with `__del__` keeps one state word
/// before its fields: bit 0 marks a completed initializer, and the remaining
/// bits count observers, which borrow the instance through a retained copy of
/// its value word and must not run its destructor.
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
                .map(|i| self.ty(i).slot_words())
                .sum::<usize>()
    }
    pub(crate) fn moved_to(&self, base: *mut u128) -> Self {
        Self { base, ..*self }
    }
    pub(crate) fn slot(&self, index: usize) -> *mut u128 {
        let offset: usize = (0..index).map(|i| self.ty(i).slot_words()).sum();
        // SAFETY: the owner reserved `words()` slots at `base`.
        unsafe { self.base.add(usize::from(self.state) + offset) }
    }
    fn state(&self) -> Option<*mut u64> {
        self.state.then_some(self.base.cast())
    }
    /// Return whether this owner must release the fields. Observers only consume
    /// their count; a live owner's hook is disarmed before calling user code.
    unsafe fn begin_release(&self, value: u128, ty: &Type) -> bool {
        unsafe {
            if let Some(state) = self.state() {
                if *state >= 2 {
                    *state -= 2;
                    return false;
                }
                if *state & 1 != 0 {
                    *state = 0;
                    let mut owner = value;
                    let hook = ty.drop.expect("class destructor");
                    memory::with_nested_drops(|| hook(&mut owner));
                    if owner != value {
                        crate::fail("destructor replaced its receiver");
                    }
                }
            }
            true
        }
    }
    /// Release the owned fields in declaration order. Inline fields finish
    /// immediately; heap fields join the destruction queue.
    unsafe fn release(&self) {
        for i in 0..self.len() {
            unsafe { release(self.slot(i).read(), self.ty(i)) };
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
            for i in 0..record.len() {
                retain(record.slot(i).read(), record.ty(i));
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
        } else if let Some(record) = ty.record(value) {
            if record.begin_release(value, ty) {
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
        } else if let Some(record) = ty.record(value) {
            if !record.begin_release(value, ty) {
                slot.write(0);
                return None;
            }
            for i in 0..record.len() {
                if let Some(child) = release_next(record.slot(i), record.ty(i)) {
                    return Some(child);
                }
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
        } else if ty.managed() {
            slot.write(0);
            return Some(value as *mut Header);
        }
        slot.write(0);
        None
    }
}

fn collection_new(ty: &'static Type) -> *mut Collection {
    try_collection_new(ty, 0).unwrap_or_else(|_| crate::fail("collection allocation failed"))
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
        let mut entry = (*c).drop_entry.or_else(|| (*c).entries.indices().next());
        while let Some(index) = entry {
            (*c).drop_entry = Some(index);
            if let Some(child) = release_next((*c).entries.slot(index, false), ty.key()) {
                memory::continue_after(header, child);
                return;
            }
            if let Some(value) = ty.value {
                if let Some(child) = release_next((*c).entries.slot(index, true), value) {
                    memory::continue_after(header, child);
                    return;
                }
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
pub(crate) fn index(index: i64, len: usize) -> usize {
    checked_index(index, len).unwrap_or_else(|| crate::fail("index out of bounds"))
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
    if ty.kind == b's' {
        let mut hash = 14695981039346656037u64;
        for &b in unsafe { strings::bytes(&(value as *const Text)) } {
            hash = (hash ^ b as u64).wrapping_mul(1099511628211);
        }
        hash
    } else {
        let mut value = value as u64;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }
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
            if self.ty().kind != b'L' {
                if let Some(i) = self.find(key) {
                    if let Some(ty) = &self.ty().value {
                        release(self.entries.get(i).value, ty);
                    }
                    release(key, self.ty().key());
                    self.entries.set(i, true, value);
                    return Ok(());
                }
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
            b'B' | b'C' => {
                if ty.kind == b'B' && ty.tag(a) != ty.tag(b) {
                    return false;
                }
                match (ty.record(a), ty.record(b)) {
                    (Some(left), Some(right)) => (0..left.len())
                        .all(|i| equal(left.slot(i).read(), right.slot(i).read(), left.ty(i))),
                    (None, None) => ty
                        .payload(a)
                        .is_none_or(|t| equal(ty.unpack(a), ty.unpack(b), t)),
                    _ => false,
                }
            }
            b'f' => f32::from_bits(a as u32) == f32::from_bits(b as u32),
            b'd' => f64::from_bits(a as u64) == f64::from_bits(b as u64),
            b's' => strings::bytes(&(a as *const Text)) == strings::bytes(&(b as *const Text)),
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
) -> Result<u128, AllocError> {
    unsafe {
        let separator = strings::utf8(&separator);
        if separator.is_empty() {
            crate::fail("string split requires a nonempty separator");
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
            retain(*value, ty);
            return Ok(());
        }
        if let Some(record) = ty.record(*value) {
            if record.state().is_some() {
                crate::fail("cannot copy a class with custom cleanup");
            }
            for i in 0..record.len() {
                let mut field = record.slot(i).read();
                if let Err(error) = try_copy_into(&mut field, record.ty(i)) {
                    // The prefix owns copies; the rest still names the source.
                    for j in (0..i).rev() {
                        release(record.slot(j).read(), record.ty(j));
                    }
                    for j in 0..record.len() {
                        record.slot(j).write(0);
                    }
                    *value = 0;
                    return Err(error);
                }
                record.slot(i).write(field);
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
            _ => {
                retain(*value, ty);
                return Ok(());
            }
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
    for i in 0..record.len() {
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
        unsafe { render(record.slot(i).read(), record.ty(i), out) };
    }
    out.push(b')');
}

unsafe fn render(value: u128, ty: &Type, out: &mut crate::render_buffer::Buffer) {
    if out.failed() {
        return;
    }
    unsafe {
        match ty.kind {
            b'B' => {
                let tuple = ty.name.starts_with("tuple[");
                if !tuple {
                    out.extend_from_slice(ty.name.as_bytes());
                    out.push(b'.');
                    let variant = &ty.variants[ty.tag(value)];
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
            b'1'..=b'4' => write!(out, "{}", value as i64).unwrap(),
            b'5'..=b'8' => write!(out, "{value}").unwrap(),
            b'f' => write!(out, "{:?}", f32::from_bits(value as u32)).unwrap(),
            b'd' => write!(out, "{:?}", f64::from_bits(value as u64)).unwrap(),
            b'v' => out.extend_from_slice(b"()"),
            b'c' => out.extend_from_slice(b"<function>"),
            b'H' => out.extend_from_slice(b"<closure>"),
            b'X' => out.extend_from_slice(b"<sender>"),
            b'Y' => out.extend_from_slice(b"<receiver>"),
            b'P' => out.extend_from_slice(b"<executor>"),
            b'Z' => out.extend_from_slice(b"<future>"),
            b'K' => out.extend_from_slice(b"<cancellation token>"),
            b'b' => out.extend_from_slice(if value == 0 { b"False" } else { b"True" }),
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

/// Narrow ABI: a live borrowed list owner, with no ownership transfer. The
/// compiler never reads Collection, Entries, or Vec layout across this boundary.
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_list_len(owner: *const u8) -> u64 {
    // SAFETY: the compiler keeps the list's owner alive through the call.
    unsafe { (*(owner.cast::<Collection>())).entries.len() as u64 }
}

/// Returns the low 64 payload bits of an integer, float, or boolean list row.
/// No pointer into the buffer escapes, including across later growing mutations.
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_list_scalar_get(owner: *const u8, i: i64) -> u64 {
    // SAFETY: generated callers supply a live list of scalars. Bounds and
    // negative indices use the same normalization as the generic dispatcher.
    unsafe {
        let collection = &*owner.cast::<Collection>();
        let i = index(i, collection.entries.len());
        collection.entries.get(i).key as u64
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_collection(
    op: i64,
    args: *const u128,
    descriptor: *const Type,
    out: *mut u128,
) {
    // SAFETY: generated code supplies three aligned inputs and an output slot
    // followed by 32 bytes of scratch range storage.
    unsafe {
        if op == 10 {
            let range = out.add(1).cast::<Range>();
            range.write(Range::new(
                *args as u64,
                *args.add(1) as u64,
                *args.add(2) as i64,
                (*descriptor).key().kind <= b'4',
            ));
            out.write(range as u128);
            return;
        }
        if op == 24 {
            out.write(0);
            let ready = plenty_generator_resume(*args as *mut Generator, out);
            out.write(wrap(out.read(), ready as u64));
            return;
        }
        out.write(collection(
            op,
            *args,
            *args.add(1),
            *args.add(2),
            descriptor,
        ));
    }
}

pub(crate) unsafe fn collection(
    op: i64,
    a: u128,
    b: u128,
    value: u128,
    descriptor: *const Type,
) -> u128 {
    // SAFETY: the independent compiler checker supplies each opcode's declared
    // types and arity. No general Rust reference is returned to generated code.
    unsafe {
        if !descriptor.is_null() && (*descriptor).kind == b'R' {
            let signed = (*descriptor).key().kind <= b'4';
            match op {
                4 | 6 => {
                    let range = &*(a as *const Range);
                    return range.at(index(b as i64, range.len as usize), signed);
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
        if !descriptor.is_null() && (*descriptor).kind == b's' && !matches!(op, 110 | 111 | 122) {
            let text = a as *const Text;
            return match op {
                5 => strings::scalar_len(text) as u128,
                12 => strings::byte_len(text) as u128,
                13 => strings::at_byte(text, b as usize) as u128,
                4 | 6 => strings::at(text, b as i64) as u128,
                7 => strings::plenty_contains(b as *const Text, text) as u128,
                _ => crate::fail("invalid string operation"),
            };
        }
        match op {
            112 => {
                let owner = *(a as *const u128);
                let c = &mut *(owner as *mut Collection);
                let position = if c.ty().kind == b'L' {
                    index(b as i64, c.entries.len())
                } else {
                    c.find(b).unwrap_or_else(|| {
                        crate::fail("dictionary key not found; use insert to add a new key")
                    })
                };
                c.entries.slot(position, c.ty().kind != b'L') as u128
            }
            114 => strings::at(a as *const Text, b as i64) as u128,
            115 | 116 => {
                let a_text = a as *const Text;
                if op == 115 {
                    return strings::at_byte(a_text, b as usize) as u128;
                }
                let source = strings::utf8(&a_text);
                let tail = &source[b as usize..];
                b + tail.chars().next().expect("valid string cursor").len_utf8() as u128
            }
            110 | 111 => {
                #[cfg(feature = "allocation-checks")]
                if op == 111
                    && (*descriptor).kind == b's'
                    && strings::bytes(&(a as *const Text)).starts_with(b"__test_")
                {
                    crate::io::plenty_println(a as *const Text);
                    return wrap(0, 0);
                }
                if op == 111 && (*descriptor).kind == b's' {
                    // A string already has its final byte representation. Printing
                    // it needs no formatting buffer, including inside destructors.
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
                if op == 111 && (*descriptor).kind == b's' {
                    out.extend_from_slice(strings::bytes(&(a as *const Text)));
                } else {
                    render(a, &*descriptor, &mut out);
                }
                if op == 111 {
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
            80 | 82 => crate::io::write_stream(a as *const Text, op == 82),
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
            65..=68 => {
                let (left, right) = (&*(a as *const Collection), &*(b as *const Collection));
                let left_entries = left.entries.iter().filter(|entry| match op {
                    65 => true,
                    66 => right.find(entry.key).is_some(),
                    _ => right.find(entry.key).is_none(),
                });
                let right_entries = right
                    .entries
                    .iter()
                    .filter(|entry| matches!(op, 65 | 68) && left.find(entry.key).is_none());
                let entries = left_entries.chain(right_entries);
                match try_set_from_entries(entries, &*left.ty) {
                    Ok(set) => wrap(set, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            62..=64 => {
                let (left, right) = (&*(a as *const Collection), &*(b as *const Collection));
                let result = match op {
                    62 => {
                        left.entries.len() <= right.entries.len()
                            && left
                                .entries
                                .iter()
                                .all(|entry| right.find(entry.key).is_some())
                    }
                    63 => {
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
            52 => strings::utf8(&(a as *const Text))
                .matches(strings::utf8(&(b as *const Text)))
                .count() as u128,
            50 | 51 => {
                let a_text = a as *const Text;
                let source = strings::utf8(&a_text);
                let b_text = b as *const Text;
                let needle = strings::utf8(&b_text);
                let found = if op == 50 {
                    source.find(needle)
                } else {
                    source.rfind(needle)
                };
                match found {
                    Some(index) => wrap(source[..index].chars().count() as u128, 1),
                    None => wrap(0, 0),
                }
            }
            48 => {
                strings::utf8(&(a as *const Text)).starts_with(strings::utf8(&(b as *const Text)))
                    as u128
            }
            49 => strings::utf8(&(a as *const Text)).ends_with(strings::utf8(&(b as *const Text)))
                as u128,
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
            36 => {
                // The compiler supplies Result[list[str], AllocError] metadata.
                let ty = (*descriptor).variants[0].fields[0];
                match try_split(a as *const Text, b as *const Text, ty) {
                    Ok(list) => wrap(list, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            }
            34 | 35 => {
                let result = if op == 34 {
                    strings::try_concat(a as *const Text, b as *const Text)
                } else {
                    let source = &*(b as *const Collection);
                    strings::try_join(
                        Some(a as *const Text),
                        source.entries.iter().map(|entry| entry.key as *const Text),
                    )
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
            0 => collection_new(&*descriptor) as u128,
            1 | 2 => {
                (*(a as *mut Collection)).insert(b, value);
                plenty_retain(a as *mut Header);
                a
            }
            3 => {
                let c = &mut *(a as *mut Collection);
                if c.ty().kind == b'L' {
                    let i = index(b as i64, c.len());
                    retain(value, c.ty().key());
                    release(c.entries.get(i).key, c.ty().key());
                    c.entries.set(i, false, value);
                } else {
                    let i = c.find(b).unwrap_or_else(|| {
                        crate::fail("dictionary key not found; use insert to add keys")
                    });
                    retain(value, c.ty().value());
                    release(c.entries.get(i).value, c.ty().value());
                    c.entries.set(i, true, value);
                }
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
            4 | 6 => {
                let c = &*(a as *const Collection);
                if op == 4 && c.ty().kind == b'D' {
                    let i = c
                        .find(b)
                        .unwrap_or_else(|| crate::fail("dictionary key not found"));
                    retain(c.entries.get(i).value, c.ty().value());
                    c.entries.get(i).value
                } else {
                    c.at(index(b as i64, c.len()))
                }
            }
            5 => (*(a as *const Collection)).len() as u128,
            15 => {
                let c = &mut *(a as *mut Collection);
                if c.ty().kind != b'L' {
                    crate::fail("owned iteration requires a list");
                }
                let i = index(b as i64, c.len());
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
            9 => {
                let mut out = crate::render_buffer::Buffer::default();
                render(
                    a,
                    if descriptor.is_null() {
                        value_type(a)
                    } else {
                        &*descriptor
                    },
                    &mut out,
                );
                out.push(b'\n');
                crate::io::output(
                    &out.finish()
                        .unwrap_or_else(|_| crate::fail("format allocation failed")),
                );
                0
            }
            117 => ranges::copy_payload(a, &*descriptor, b as *mut Range),
            118 => match strings::try_c_string(a as *const Text) {
                Ok(text) => wrap(text as u128, 0),
                Err(strings::CStrError::EmbeddedNul) => wrap(wrap(0, 0), 1),
                Err(strings::CStrError::Allocation(error)) => {
                    wrap(wrap(wrap(0, error as u64), 1), 1)
                }
            },
            11 => try_dictionary_snapshot(&mut *(a as *mut Collection), &*descriptor, true)
                .unwrap_or_else(|_| crate::fail("dictionary snapshot allocation failed")),
            119 => match try_box_new(&*descriptor, a) {
                Ok(boxed) => wrap(boxed as u128, 0),
                Err(error) => {
                    // A failed box drops the value it would have owned.
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

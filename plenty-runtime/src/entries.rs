//! Type-sized collection rows. Dense scalar lists use their native widths;
//! other rows keep aligned slots and owner-local inline payloads.
use crate::aggregates::Type;
use crate::entry_order::Order;
use crate::{memory, ranges};
use std::cmp::Ordering;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

mod sort;

#[cfg(test)]
mod tests;

pub(crate) fn scalar_bytes(ty: &Type) -> Option<usize> {
    match ty.kind {
        b'1' | b'5' | b'b' => Some(1),
        b'2' | b'6' => Some(2),
        b'3' | b'7' | b'f' => Some(4),
        b'4' | b'8' | b'd' => Some(8),
        _ => None,
    }
}

fn canonical_scalar(value: u128, ty: &Type) -> u128 {
    match ty.kind {
        b'1' => value as i8 as i64 as u64 as u128,
        b'2' => value as i16 as i64 as u64 as u128,
        b'3' => value as i32 as i64 as u64 as u128,
        _ => value,
    }
}

/// Decode only a scalar's declared bytes, restoring the canonical signed low
/// word used by runtime arithmetic, formatting, hashing and comparisons.
pub(crate) unsafe fn read_scalar(base: *const u8, ty: &Type) -> u128 {
    unsafe {
        let bits = match ty.kind {
            b'1' => base.cast::<i8>().read() as i64 as u64,
            b'2' => base.cast::<i16>().read() as i64 as u64,
            b'3' => base.cast::<i32>().read() as i64 as u64,
            b'5' | b'b' => base.read() as u64,
            b'6' => base.cast::<u16>().read() as u64,
            b'7' | b'f' => base.cast::<u32>().read() as u64,
            b'4' | b'8' | b'd' => base.cast::<u64>().read(),
            _ => unreachable!("scalar list element"),
        };
        bits as u128
    }
}

unsafe fn write_scalar(base: *mut u8, value: u128, ty: &Type) {
    unsafe {
        match scalar_bytes(ty).expect("scalar list element") {
            1 => base.write(value as u8),
            2 => base.cast::<u16>().write(value as u16),
            4 => base.cast::<u32>().write(value as u32),
            8 => base.cast::<u64>().write(value as u64),
            _ => unreachable!(),
        }
    }
}

unsafe fn read_row(base: *const u8, key: &Type, value: Option<&Type>, packed: bool) -> Entry {
    unsafe {
        Entry {
            key: if packed {
                read_scalar(base, key)
            } else {
                base.cast::<u128>().read()
            },
            value: value.map_or(0, |_| base.add(key.slot_words() * 16).cast::<u128>().read()),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Entry {
    pub key: u128,
    pub value: u128,
}

/// A byte buffer backed by `Vec<u128>` for aligned inline payloads. Length and
/// capacity count bytes; allocation rounds up to whole words. Generated code
/// reads the pointer and byte count through `list_layout`.
#[repr(C)]
pub(crate) struct Words {
    ptr: NonNull<u128>,
    len: usize,
    capacity: usize,
}

impl Words {
    const fn new() -> Self {
        Self {
            ptr: NonNull::dangling(),
            len: 0,
            capacity: 0,
        }
    }
    fn try_reserve(&mut self, additional: usize) -> Result<(), memory::AllocError> {
        let needed = self
            .len
            .checked_add(additional)
            .ok_or(memory::AllocError::CapacityOverflow)?;
        let words = needed.div_ceil(16);
        // SAFETY: the allocation belongs to Vec<u128>. A zero length avoids
        // reading padding beyond the initialized byte prefix. ManuallyDrop
        // keeps ownership here if reservation fails or unwinds.
        let mut vec = ManuallyDrop::new(unsafe {
            Vec::from_raw_parts(self.ptr.as_ptr(), 0, self.capacity / 16)
        });
        let result = memory::try_reserve(&mut vec, words);
        // SAFETY: a Vec's pointer is never null.
        self.ptr = unsafe { NonNull::new_unchecked(vec.as_mut_ptr()) };
        self.capacity = vec.capacity() * 16;
        result
    }
    fn capacity(&self) -> usize {
        self.capacity
    }
    fn clear(&mut self) {
        self.truncate(0);
    }
    fn truncate(&mut self, len: usize) {
        // Words are plain integers; payload owners are released by callers.
        self.len = self.len.min(len);
    }
    fn resize(&mut self, len: usize, value: u8) {
        assert!(len <= self.capacity);
        if len > self.len {
            // SAFETY: the reserved suffix is exclusively owned and writable.
            unsafe {
                self.ptr
                    .as_ptr()
                    .cast::<u8>()
                    .add(self.len)
                    .write_bytes(value, len - self.len)
            };
        }
        self.len = len;
    }
}

// SAFETY: `Words` owns its buffer exactly as `Vec<u128>` does.
unsafe impl Send for Words {}
// SAFETY: as for `Send`.
unsafe impl Sync for Words {}

impl Deref for Words {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        // SAFETY: `len` bytes from `ptr` are initialized.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr().cast(), self.len) }
    }
}

impl DerefMut for Words {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: as for `deref`, and `&mut self` is exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast(), self.len) }
    }
}

impl Drop for Words {
    fn drop(&mut self) {
        // SAFETY: the parts describe this buffer's allocation, as in `with`.
        drop(unsafe { Vec::from_raw_parts(self.ptr.as_ptr(), 0, self.capacity / 16) });
    }
}

#[repr(C)]
pub(crate) struct Entries {
    data: Words,
    /// A removed dense row stays initialized at the end until the next mutation.
    /// Its owners have transferred to the caller; it is not a live member.
    removed: bool,
    key: &'static Type,
    value: Option<&'static Type>,
    stride: usize,
    order: Option<Order>,
}

const _: () = {
    assert!(std::mem::offset_of!(Entries, data) == 0);
    assert!(std::mem::offset_of!(Words, ptr) == 0);
    assert!(std::mem::offset_of!(Words, len) == 8);
    assert!(std::mem::offset_of!(Entries, removed) == 24);
};

impl Entries {
    pub fn new(key: &'static Type, value: Option<&'static Type>) -> Self {
        Self {
            data: Words::new(),
            key,
            value,
            stride: if value.is_none() {
                scalar_bytes(key).unwrap_or(key.slot_words() * 16)
            } else {
                (key.slot_words() + value.map_or(0, Type::slot_words)) * 16
            },
            removed: false,
            order: None,
        }
    }
    pub fn ordered(key: &'static Type, value: Option<&'static Type>) -> Self {
        Self {
            order: Some(Order::new()),
            // Hash rows retain canonical slots, including an unused value slot
            // for sets; their references and ownership traversal share this ABI.
            stride: (key.slot_words() + value.map_or(1, Type::slot_words)) * 16,
            ..Self::new(key, value)
        }
    }
    pub fn len(&self) -> usize {
        self.order.as_ref().map_or_else(
            || self.data.len() / self.stride - usize::from(self.removed),
            Order::len,
        )
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn capacity(&self) -> usize {
        let rows = self.data.capacity() / self.stride;
        self.order
            .as_ref()
            .map_or(rows, |order| rows.min(order.capacity()))
    }
    pub fn row_bytes(&self) -> usize {
        self.stride
    }
    pub fn get(&self, index: usize) -> Entry {
        assert!(index < self.data.len() / self.stride);
        debug_assert!(
            self.order
                .as_ref()
                .map_or(index < self.len(), |order| order.contains(index))
        );
        self.read_row(index)
    }
    /// A dense row's key word, without the value lookup `get` performs.
    fn key_at(&self, index: usize) -> u128 {
        self.read_row(index).key
    }
    fn packed(&self) -> bool {
        self.order.is_none() && self.value.is_none() && scalar_bytes(self.key).is_some()
    }
    fn read_row(&self, index: usize) -> Entry {
        // SAFETY: callers only name initialized live or extraction rows.
        unsafe {
            read_row(
                self.data.as_ptr().add(index * self.stride),
                self.key,
                self.value,
                self.packed(),
            )
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Entry> + ExactSizeIterator + Clone + '_ {
        self.indices().map(|i| self.get(i))
    }
    pub fn indices(
        &self,
    ) -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator + Clone + '_ {
        // Exactly one side is populated; chaining keeps both representations
        // allocation-free and preserves double-ended traversal.
        let dense = 0..if self.order.is_none() { self.len() } else { 0 };
        let sparse = self.order.iter().flat_map(Order::indices);
        EntryIndices {
            inner: dense.chain(sparse),
            remaining: self.len(),
        }
    }
    pub fn vacant(&self) -> usize {
        self.order
            .as_ref()
            .map_or_else(|| self.len(), Order::vacant)
    }
    pub fn first(&self) -> Option<usize> {
        self.order.as_ref().expect("hash entries").first()
    }
    pub fn next(&self, index: usize) -> Option<usize> {
        self.order.as_ref().expect("hash entries").next(index)
    }
    /// Dense row `i` is at byte offset `i * row_bytes()` from this address,
    /// as `slot(i, false)` returns, until the next mutation.
    pub fn rows(&mut self) -> *mut u128 {
        debug_assert!(self.order.is_none());
        self.data.as_mut_ptr().cast()
    }
    /// The address of a typed element, not necessarily a complete u128 slot:
    /// packed scalar rows have only their declared width/alignment. Callers
    /// dereferencing this pointer must use the element's storage representation.
    pub fn slot(&mut self, index: usize, value: bool) -> *mut u128 {
        assert!(index < self.data.len() / self.stride);
        debug_assert!(
            self.order
                .as_ref()
                .map_or(index < self.len(), |order| order.contains(index))
        );
        unsafe {
            self.data
                .as_mut_ptr()
                .add(index * self.stride + if value { self.key.slot_words() * 16 } else { 0 })
                .cast()
        }
    }
    pub unsafe fn set(&mut self, index: usize, value_field: bool, value: u128) {
        if value_field && self.value.is_none() {
            return;
        }
        let ty = if value_field {
            self.value
        } else {
            Some(self.key)
        };
        let slot = self.slot(index, value_field);
        unsafe {
            if self.packed() {
                write_scalar(slot.cast(), value, self.key);
            } else if let Some(ty) = ty {
                ranges::store(slot, value, ty);
            } else {
                slot.write(value);
            }
        }
    }
    /// Forget transferred ownership without following an inline pointer that
    /// already belongs to another owner. Scalar rows clear only their own bytes.
    pub unsafe fn forget_key(&mut self, index: usize) {
        let slot = self.slot(index, false);
        unsafe {
            if self.packed() {
                write_scalar(slot.cast(), 0, self.key);
            } else {
                slot.write(0);
            }
        }
    }
    fn relocate(&mut self, dense_start: usize) {
        if self.key.payload_bytes() == 0 && self.value.is_none_or(|ty| ty.payload_bytes() == 0) {
            return;
        }
        // Include an extracted row: its inline address must survive shifting
        // without allocating scratch storage or retaining its transferred owners.
        // Dead hash rows may contain already-freed owners. Never relocate them.
        let base = self.data.as_mut_ptr();
        let dense_end = if self.order.is_none() {
            self.data.len() / self.stride
        } else {
            dense_start
        };
        let sparse = self.order.iter().flat_map(Order::indices);
        for i in (dense_start..dense_end).chain(sparse) {
            unsafe {
                let slot = base.add(i * self.stride).cast::<u128>();
                ranges::relocate(slot, self.key);
                if let Some(ty) = self.value {
                    ranges::relocate(slot.add(self.key.slot_words()), ty);
                }
            }
        }
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), memory::AllocError> {
        if let Some(order) = &mut self.order {
            let needed = order
                .len()
                .checked_add(additional)
                .ok_or(memory::AllocError::CapacityOverflow)?;
            order.try_reserve(additional)?;
            let bytes = needed
                .saturating_sub(self.data.len() / self.stride)
                .checked_mul(self.stride)
                .ok_or(memory::AllocError::CapacityOverflow)?;
            let previous = self.data.as_ptr();
            self.data.try_reserve(bytes)?;
            if previous != self.data.as_ptr() {
                self.relocate(0);
            }
            return Ok(());
        }
        // Extraction scratch is dead at the next mutation. Discard it before
        // computing capacity so pop followed by append can reuse the freed row.
        self.data.truncate(self.len() * self.stride);
        self.removed = false;
        let bytes = additional
            .checked_mul(self.stride)
            .ok_or(memory::AllocError::CapacityOverflow)?;
        let previous = self.data.as_ptr();
        self.data.try_reserve(bytes)?;
        if previous != self.data.as_ptr() {
            self.relocate(0);
        }
        Ok(())
    }
    /// Reservation belongs to the surrounding transactional operation.
    pub unsafe fn push(&mut self, entry: Entry) {
        assert!(self.len() < self.capacity());
        let index = self.vacant();
        if let Some(order) = &mut self.order {
            order.push();
            if index == self.data.len() / self.stride {
                self.data.resize(self.data.len() + self.stride, 0);
            }
        } else {
            self.data.truncate(index * self.stride);
            self.removed = false;
            self.data.resize(self.data.len() + self.stride, 0);
        }
        unsafe {
            self.set(index, false, entry.key);
            self.set(index, true, entry.value);
        }
    }
    pub fn clear(&mut self) {
        self.data.clear();
        self.removed = false;
        if let Some(order) = &mut self.order {
            order.clear();
        }
    }
    pub fn drain(&mut self) -> impl Iterator<Item = Entry> + '_ {
        let count = self.len();
        let base = self.data.as_ptr();
        let stride = self.stride;
        let key = self.key;
        let value = self.value;
        let packed = self.packed();
        self.data.clear();
        self.removed = false;
        let dense = 0..if self.order.is_none() { count } else { 0 };
        let sparse = self.order.as_mut().map(Order::drain).into_iter().flatten();
        // The reserved buffer stays live and its trivially copied words remain
        // initialized. The returned iterator borrows self, preventing reallocation.
        dense
            .chain(sparse)
            .map(move |i| unsafe { read_row(base.add(i * stride), key, value, packed) })
    }
    pub unsafe fn append(&mut self, source: &mut Self) {
        for entry in source.iter() {
            unsafe {
                self.push(entry);
            }
        }
        source.clear();
    }
    /// The removed inline payload remains valid until the next mutation or drop.
    /// Native callers snapshot it before releasing or mutating this collection.
    pub unsafe fn remove(&mut self, index: usize) -> Entry {
        let entry = self.get(index);
        if let Some(order) = &mut self.order {
            order.remove(index);
            return entry;
        }
        assert!(index < self.len());
        self.data.truncate(self.len() * self.stride);
        self.removed = true;
        // The tail's payload already occupies extraction scratch. No row moves,
        // so neither rotation nor address repair is needed.
        if index == self.len() {
            return entry;
        }
        let start = index * self.stride;
        // Rotation leaves every surviving row in order, and moves the extracted
        // row into existing storage at the end. This works for arbitrary layouts.
        self.data[start..].rotate_left(self.stride);
        self.relocate(index);
        let extracted = self.len() * self.stride;
        self.read_row(extracted / self.stride)
    }
    pub fn reverse(&mut self) {
        assert!(self.order.is_none(), "only dense lists reverse");
        for i in 0..self.len() / 2 {
            let j = self.len() - 1 - i;
            self.exchange_bytes(i, j);
        }
        self.relocate(0);
    }
    /// Exchange two dense rows of any layout without allocating.
    pub fn swap(&mut self, i: usize, j: usize) {
        assert!(self.order.is_none(), "only dense lists swap");
        assert!(i < self.len() && j < self.len());
        self.exchange(i, j, Relocation::of(self.key));
    }
    fn exchange(&mut self, i: usize, j: usize, relocation: Relocation) {
        if i == j {
            return;
        }
        self.exchange_bytes(i, j);
        // Each row is read again before the next exchange, so its inline
        // address must be valid immediately rather than after a final pass.
        let base = self.data.as_mut_ptr();
        for row in [i, j] {
            // SAFETY: both rows are in bounds and hold live values of `key`.
            unsafe {
                let slot = base.add(row * self.stride).cast::<u128>();
                match relocation {
                    Relocation::None => {}
                    Relocation::Flat => ranges::relocate_flat(slot),
                    Relocation::General => ranges::relocate(slot, self.key),
                }
            }
        }
    }
    fn exchange_bytes(&mut self, i: usize, j: usize) {
        let (i, j) = if i < j { (i, j) } else { (j, i) };
        let (left, right) = self.data.split_at_mut(j * self.stride);
        left[i * self.stride..(i + 1) * self.stride].swap_with_slice(&mut right[..self.stride]);
    }
    /// Sort dense rows in place without allocating. Rows that compare equal may
    /// change relative order, so `compare` must not equate distinguishable rows.
    pub fn sort_unstable_by(&mut self, compare: impl FnMut(u128, u128) -> Ordering) {
        assert!(self.order.is_none(), "only dense lists sort");
        fn sort<const N: usize>(
            data: &mut [u8],
            key: &Type,
            packed: bool,
            mut compare: impl FnMut(u128, u128) -> Ordering,
        ) {
            let (rows, _) = data.as_chunks_mut::<N>();
            rows.sort_unstable_by(|a, b| {
                // These non-inline rows have the selected scalar width
                // or a complete aligned canonical slot. Array references used
                // by sorting may point to its temporary copies, so decode
                // unaligned bytes rather than assuming the buffer's alignment.
                let read = |row: &[u8; N]| {
                    let mut word = [0u8; 16];
                    word[..N].copy_from_slice(row);
                    let value = u128::from_ne_bytes(word);
                    if packed {
                        canonical_scalar(value, key)
                    } else {
                        value
                    }
                };
                compare(read(a), read(b))
            });
        }
        let len = self.len();
        let packed = self.packed();
        let data = &mut self.data[..len * self.stride];
        match self.stride {
            1 => return sort::<1>(data, self.key, packed, compare),
            2 => return sort::<2>(data, self.key, packed, compare),
            4 => return sort::<4>(data, self.key, packed, compare),
            8 => return sort::<8>(data, self.key, packed, compare),
            16 => return sort::<16>(data, self.key, packed, compare),
            _ => {}
        }
        self.sort_unstable_rows_by(compare);
    }
    /// Sort dense rows of any layout with this module's introsort. Unlike Rust's
    /// slice sort, it never panics on an inconsistent `compare`, which makes it
    /// suitable for user-defined keys; every row remains exactly once.
    pub fn sort_unstable_rows_by(&mut self, mut compare: impl FnMut(u128, u128) -> Ordering) {
        assert!(self.order.is_none(), "only dense lists sort");
        let len = self.len();
        // Partitioning an already ordered run needlessly destroys its locality.
        // Stop this probe at the first change of direction on mixed input.
        let relocation = Relocation::of(self.key);
        let mut direction = Ordering::Equal;
        let monotonic = (1..len).all(|i| {
            let next = compare(self.key_at(i - 1), self.key_at(i));
            if direction == Ordering::Equal {
                direction = next;
            }
            next == Ordering::Equal || next == direction
        });
        if monotonic {
            if direction == Ordering::Greater {
                self.reverse();
            }
            return;
        }
        let depth = 2 * len.checked_ilog2().unwrap_or(0);
        self.sort_rows(0, len, depth, relocation, &mut compare);
    }

    fn heap_sort_rows(
        &mut self,
        start: usize,
        len: usize,
        relocation: Relocation,
        compare: &mut impl FnMut(u128, u128) -> Ordering,
    ) {
        let mut sift = |entries: &mut Self, mut root: usize, end: usize| loop {
            let mut child = 2 * root + 1;
            if child >= end {
                break;
            }
            if child + 1 < end
                && compare(
                    entries.key_at(start + child),
                    entries.key_at(start + child + 1),
                )
                .is_lt()
            {
                child += 1;
            }
            if compare(entries.key_at(start + root), entries.key_at(start + child)).is_ge() {
                break;
            }
            entries.exchange(start + root, start + child, relocation);
            root = child;
        };
        for root in (0..len / 2).rev() {
            sift(self, root, len);
        }
        for end in (1..len).rev() {
            self.exchange(start, start + end, relocation);
            sift(self, 0, end);
        }
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&Entry) -> bool) {
        if self.order.is_some() {
            let mut current = self.first();
            while let Some(index) = current {
                current = self.next(index);
                if !keep(&self.get(index)) {
                    // SAFETY: this is a live stable slot; the predicate has
                    // handled its ownership, and unlinking only changes links.
                    unsafe {
                        self.remove(index);
                    }
                }
            }
            return;
        }
        let mut target = 0;
        for source in 0..self.len() {
            if keep(&self.get(source)) {
                if source != target {
                    let start = source * self.stride;
                    self.data
                        .copy_within(start..start + self.stride, target * self.stride);
                }
                target += 1;
            }
        }
        self.data.truncate(target * self.stride);
        self.removed = false;
        self.relocate(0);
    }
}

#[derive(Clone)]
struct EntryIndices<I> {
    inner: I,
    remaining: usize,
}
impl<I: Iterator<Item = usize>> Iterator for EntryIndices<I> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        let item = self.inner.next()?;
        self.remaining -= 1;
        Some(item)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl<I: DoubleEndedIterator<Item = usize>> DoubleEndedIterator for EntryIndices<I> {
    fn next_back(&mut self) -> Option<usize> {
        let item = self.inner.next_back()?;
        self.remaining -= 1;
        Some(item)
    }
}
impl<I: Iterator<Item = usize>> ExactSizeIterator for EntryIndices<I> {}

/// How a moved row repairs the inline address in its value word, decided once
/// per sort instead of walking the descriptor on every exchange.
#[derive(Clone, Copy)]
enum Relocation {
    None,
    /// A record whose fields have no inline storage of their own.
    Flat,
    General,
}
impl Relocation {
    fn of(ty: &Type) -> Self {
        if ty.payload_bytes() == 0 {
            Self::None
        } else if ranges::flat_record(ty) {
            Self::Flat
        } else {
            Self::General
        }
    }
}

//! Type-sized collection rows. Scalar rows retain their existing 32-byte size;
//! Inline payloads live in the same buffer as their containing row.
use crate::aggregates::Type;
use crate::entry_order::Order;
use crate::{memory, ranges};
use std::cmp::Ordering;

mod sort;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(crate) struct Entry {
    pub key: u128,
    pub value: u128,
}

pub(crate) struct Entries {
    data: Vec<u128>,
    key: &'static Type,
    value: Option<&'static Type>,
    stride: usize,
    /// A removed dense row stays initialized at the end until the next mutation.
    /// Its owners have transferred to the caller; it is not a live member.
    removed: bool,
    order: Option<Order>,
}

impl Entries {
    pub fn new(key: &'static Type, value: Option<&'static Type>) -> Self {
        Self {
            data: Vec::new(),
            key,
            value,
            stride: key.slot_words() + value.map_or(1, Type::slot_words),
            removed: false,
            order: None,
        }
    }
    pub fn ordered(key: &'static Type, value: Option<&'static Type>) -> Self {
        Self {
            order: Some(Order::new()),
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
        self.stride * 16
    }
    pub fn get(&self, index: usize) -> Entry {
        assert!(index < self.data.len() / self.stride);
        debug_assert!(
            self.order
                .as_ref()
                .map_or(index < self.len(), |order| order.contains(index))
        );
        Entry {
            key: self.data[index * self.stride],
            value: self.data[index * self.stride + self.key.slot_words()],
        }
    }
    /// A dense row's key word, without the value lookup `get` performs.
    fn key_at(&self, index: usize) -> u128 {
        self.data[index * self.stride]
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
    /// Dense row `i` is at `rows() + i * row_bytes()`, as `slot(i, false)`
    /// returns, until the next mutation.
    pub fn rows(&mut self) -> *mut u128 {
        debug_assert!(self.order.is_none());
        self.data.as_mut_ptr()
    }
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
                .add(index * self.stride + if value { self.key.slot_words() } else { 0 })
        }
    }
    pub unsafe fn set(&mut self, index: usize, value_field: bool, value: u128) {
        let ty = if value_field {
            self.value
        } else {
            Some(self.key)
        };
        let slot = self.slot(index, value_field);
        unsafe {
            if let Some(ty) = ty {
                ranges::store(slot, value, ty);
            } else {
                slot.write(value);
            }
        }
    }
    fn relocate(&mut self, dense_start: usize) {
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
                let slot = base.add(i * self.stride);
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
            let words = needed
                .saturating_sub(self.data.len() / self.stride)
                .checked_mul(self.stride)
                .ok_or(memory::AllocError::CapacityOverflow)?;
            let previous = self.data.as_ptr();
            memory::try_reserve(&mut self.data, words)?;
            if previous != self.data.as_ptr() {
                self.relocate(0);
            }
            return Ok(());
        }
        // Extraction scratch is dead at the next mutation. Discard it before
        // computing capacity so pop followed by append can reuse the freed row.
        self.data.truncate(self.len() * self.stride);
        self.removed = false;
        let words = additional
            .checked_mul(self.stride)
            .ok_or(memory::AllocError::CapacityOverflow)?;
        let previous = self.data.as_ptr();
        memory::try_reserve(&mut self.data, words)?;
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
        let value_offset = self.key.slot_words();
        self.data.clear();
        self.removed = false;
        let dense = 0..if self.order.is_none() { count } else { 0 };
        let sparse = self.order.as_mut().map(Order::drain).into_iter().flatten();
        // The reserved buffer stays live and its trivially copied words remain
        // initialized. The returned iterator borrows self, preventing reallocation.
        dense.chain(sparse).map(move |i| unsafe {
            Entry {
                key: *base.add(i * stride),
                value: *base.add(i * stride + value_offset),
            }
        })
    }
    /// Copy dense rows `start..start + count` to the end of `target`, which
    /// must have reserved room and the same row layout. The rows still belong
    /// to `self` until the caller calls `forget_all`, so each owner is moved
    /// exactly once: one word copy per row, no per-item push.
    pub unsafe fn copy_rows_to(&self, start: usize, count: usize, target: &mut Self) {
        debug_assert!(self.order.is_none() && target.order.is_none());
        debug_assert_eq!(self.stride, target.stride);
        assert!(start + count <= self.len());
        assert!(target.len() + count <= target.capacity());
        target.data.truncate(target.len() * target.stride);
        target.removed = false;
        let dense_start = target.len();
        target
            .data
            .extend_from_slice(&self.data[start * self.stride..(start + count) * self.stride]);
        // Inline payload addresses point into the source rows.
        if self.key.slot_words() > 1 {
            target.relocate(dense_start);
        }
    }
    /// Drop the rows without releasing their owners, after `copy_rows_to`
    /// moved every one of them elsewhere.
    pub fn forget_all(&mut self) {
        debug_assert!(self.order.is_none());
        self.data.clear();
        self.removed = false;
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
        Entry {
            key: self.data[extracted],
            value: self.data[extracted + self.key.slot_words()],
        }
    }
    pub fn reverse(&mut self) {
        assert!(self.order.is_none(), "only dense lists reverse");
        for i in 0..self.len() / 2 {
            let j = self.len() - 1 - i;
            for word in 0..self.stride {
                self.data
                    .swap(i * self.stride + word, j * self.stride + word);
            }
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
        for word in 0..self.stride {
            self.data
                .swap(i * self.stride + word, j * self.stride + word);
        }
        // Each row is read again before the next exchange, so its inline
        // address must be valid immediately rather than after a final pass.
        let base = self.data.as_mut_ptr();
        for row in [i, j] {
            // SAFETY: both rows are in bounds and hold live values of `key`.
            unsafe {
                let slot = base.add(row * self.stride);
                match relocation {
                    Relocation::None => {}
                    Relocation::Flat => ranges::relocate_flat(slot),
                    Relocation::General => ranges::relocate(slot, self.key),
                }
            }
        }
    }
    /// Sort dense rows in place without allocating. Rows that compare equal may
    /// change relative order, so `compare` must not equate distinguishable rows.
    pub fn sort_unstable_by(&mut self, mut compare: impl FnMut(u128, u128) -> Ordering) {
        assert!(self.order.is_none(), "only dense lists sort");
        let len = self.len();
        if self.stride == 2 {
            // Scalar rows hold their value in the first of two words.
            let (rows, _) = self.data[..len * 2].as_chunks_mut::<2>();
            rows.sort_unstable_by(|a, b| compare(a[0], b[0]));
            return;
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

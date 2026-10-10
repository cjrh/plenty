//! Type-sized collection rows. Scalar rows retain their existing 32-byte size;
//! Inline payloads live in the same buffer as their containing row.
use crate::aggregates::Type;
use crate::entry_order::Order;
use crate::{memory, ranges};

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
        debug_assert!(self
            .order
            .as_ref()
            .map_or(index < self.len(), |order| order.contains(index)));
        Entry {
            key: self.data[index * self.stride],
            value: self.data[index * self.stride + self.key.slot_words()],
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
    pub fn slot(&mut self, index: usize, value: bool) -> *mut u128 {
        assert!(index < self.data.len() / self.stride);
        debug_assert!(self
            .order
            .as_ref()
            .map_or(index < self.len(), |order| order.contains(index)));
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
    fn relocate(&mut self) {
        // Include an extracted row: its inline address must survive shifting
        // without allocating scratch storage or retaining its transferred owners.
        // Dead hash rows may contain already-freed owners. Never relocate them.
        let base = self.data.as_mut_ptr();
        for i in self.indices().chain(self.removed.then_some(self.len())) {
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
                self.relocate();
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
            self.relocate();
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
        let start = index * self.stride;
        // Rotation leaves every surviving row in order, and moves the extracted
        // row into existing storage at the end. This works for arbitrary layouts.
        self.data[start..].rotate_left(self.stride);
        self.removed = true;
        self.relocate();
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
        self.relocate();
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
        self.relocate();
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

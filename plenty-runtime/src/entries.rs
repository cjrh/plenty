//! Type-sized collection rows. Scalar rows retain their existing 32-byte size;
//! range payloads live in the same buffer as their containing row.
use crate::aggregates::Type;
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
    scratch: [u128; 2],
}

impl Entries {
    pub fn new(key: &'static Type, value: Option<&'static Type>) -> Self {
        Self {
            data: Vec::new(),
            key,
            value,
            stride: key.slot_words() + value.map_or(1, Type::slot_words),
            scratch: [0; 2],
        }
    }
    pub fn len(&self) -> usize {
        self.data.len() / self.stride
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn capacity(&self) -> usize {
        self.data.capacity() / self.stride
    }
    pub fn row_bytes(&self) -> usize {
        self.stride * 16
    }
    pub fn get(&self, index: usize) -> Entry {
        assert!(index < self.len());
        Entry {
            key: self.data[index * self.stride],
            value: self.data[index * self.stride + self.key.slot_words()],
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Entry> + ExactSizeIterator + Clone + '_ {
        (0..self.len()).map(|i| self.get(i))
    }
    pub fn slot(&mut self, index: usize, value: bool) -> *mut u128 {
        assert!(index < self.len());
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
        for i in 0..self.len() {
            unsafe {
                ranges::relocate(self.slot(i, false), self.key);
                if let Some(ty) = self.value {
                    ranges::relocate(self.slot(i, true), ty);
                }
            }
        }
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), memory::AllocError> {
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
        let index = self.len();
        self.data.resize(self.data.len() + self.stride, 0);
        unsafe {
            self.set(index, false, entry.key);
            self.set(index, true, entry.value);
        }
    }
    pub fn clear(&mut self) {
        self.data.clear();
    }
    pub fn drain(&mut self) -> impl Iterator<Item = Entry> + '_ {
        let count = self.len();
        let base = self.data.as_ptr();
        let stride = self.stride;
        let value_offset = self.key.slot_words();
        self.data.clear();
        // The reserved buffer stays live and its trivially copied words remain
        // initialized. The returned iterator borrows self, preventing reallocation.
        (0..count).map(move |i| unsafe {
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
    /// The removed inline payload remains valid until the next removal or drop.
    /// Native callers snapshot it before releasing or mutating this collection.
    pub unsafe fn remove(&mut self, index: usize) -> Entry {
        let mut entry = self.get(index);
        unsafe {
            if self.key.has_inline_range() {
                entry.key =
                    ranges::copy_payload(entry.key, self.key, self.scratch.as_mut_ptr().cast());
            } else if let Some(ty) = self.value.filter(|t| t.has_inline_range()) {
                entry.value =
                    ranges::copy_payload(entry.value, ty, self.scratch.as_mut_ptr().cast());
            }
        }
        let start = index * self.stride;
        self.data.copy_within(start + self.stride.., start);
        self.data.truncate(self.data.len() - self.stride);
        self.relocate();
        // Methods taking &mut self above invalidate the earlier scratch borrow.
        // Expose a fresh address only after the final whole-owner reborrow.
        if self.key.active_range(entry.key) {
            entry.key = (entry.key & !(u64::MAX as u128)) | self.scratch.as_mut_ptr() as u128;
        } else if self.value.is_some_and(|ty| ty.active_range(entry.value)) {
            entry.value = (entry.value & !(u64::MAX as u128)) | self.scratch.as_mut_ptr() as u128;
        }
        entry
    }
    pub fn reverse(&mut self) {
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
        self.relocate();
    }
}

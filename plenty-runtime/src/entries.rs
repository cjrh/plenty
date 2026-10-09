//! Type-sized collection rows. Scalar rows retain their existing 32-byte size;
//! Inline payloads live in the same buffer as their containing row.
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
    /// A removed row stays initialized at the end until the next mutation.
    /// Its owners have transferred to the caller; it is not a live member.
    removed: bool,
}

impl Entries {
    pub fn new(key: &'static Type, value: Option<&'static Type>) -> Self {
        Self {
            data: Vec::new(),
            key,
            value,
            stride: key.slot_words() + value.map_or(1, Type::slot_words),
            removed: false,
        }
    }
    pub fn len(&self) -> usize {
        self.data.len() / self.stride - usize::from(self.removed)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
        // Include an extracted row: its inline address must survive shifting
        // without allocating scratch storage or retaining its transferred owners.
        for i in 0..self.data.len() / self.stride {
            unsafe {
                let slot = self.data.as_mut_ptr().add(i * self.stride);
                ranges::relocate(slot, self.key);
                if let Some(ty) = self.value {
                    ranges::relocate(slot.add(self.key.slot_words()), ty);
                }
            }
        }
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), memory::AllocError> {
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
        let index = self.len();
        self.data.truncate(index * self.stride);
        self.removed = false;
        self.data.resize(self.data.len() + self.stride, 0);
        unsafe {
            self.set(index, false, entry.key);
            self.set(index, true, entry.value);
        }
    }
    pub fn clear(&mut self) {
        self.data.clear();
        self.removed = false;
    }
    pub fn drain(&mut self) -> impl Iterator<Item = Entry> + '_ {
        let count = self.len();
        let base = self.data.as_ptr();
        let stride = self.stride;
        let value_offset = self.key.slot_words();
        self.data.clear();
        self.removed = false;
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
    /// The removed inline payload remains valid until the next mutation or drop.
    /// Native callers snapshot it before releasing or mutating this collection.
    pub unsafe fn remove(&mut self, index: usize) -> Entry {
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
        self.removed = false;
        self.relocate();
    }
}

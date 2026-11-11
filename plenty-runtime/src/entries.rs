//! Type-sized collection rows. Dense scalar lists use their native widths;
//! other rows keep aligned slots and owner-local inline payloads.
use crate::aggregates::Type;
use crate::entry_order::Order;
use crate::{memory, ranges};
use std::cmp::Ordering;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

pub(crate) mod enum_bits;
pub(crate) mod sort;

#[cfg(test)]
mod tests;

pub(crate) fn scalar_bytes(ty: &Type) -> Option<usize> {
    crate::list_layout::scalar_bytes(ty.kind)
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

/// The element word of the dense list row at `base`, as `Entries::get`
/// returns it. Slice views address rows without their list.
pub(crate) unsafe fn dense_element(base: *const u8, element: &Type) -> u128 {
    unsafe { read_row(base, element, None, scalar_bytes(element).is_some()).key }
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
    /// The buffer's first byte, derived from the owning pointer rather than
    /// from a slice reference, so callers holding only `&Words` may return it
    /// for a write that the compiler's exclusive loan authorizes.
    fn base(&self) -> *mut u8 {
        self.ptr.as_ptr().cast()
    }
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
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        assert!(bytes.len() <= self.capacity - self.len);
        // SAFETY: the reserved suffix is exclusively owned and writable, and
        // `bytes` cannot alias it.
        unsafe {
            self.ptr
                .as_ptr()
                .cast::<u8>()
                .add(self.len)
                .copy_from_nonoverlapping(bytes.as_ptr(), bytes.len())
        };
        self.len += bytes.len();
    }
    /// Move bytes `at..` up by `width` with one `memmove` and zero the
    /// opened gap. Unlike `resize` followed by `copy_within`, the new tail is
    /// written once, by the move.
    fn open_gap(&mut self, at: usize, width: usize) {
        assert!(at <= self.len && width <= self.capacity - self.len);
        // SAFETY: both ranges lie in the owned allocation, `copy_to` permits
        // overlap, and zeroing the gap initializes every byte below the new
        // length.
        unsafe {
            let gap = self.ptr.as_ptr().cast::<u8>().add(at);
            gap.copy_to(gap.add(width), self.len - at);
            gap.write_bytes(0, width);
        }
        self.len += width;
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
    order: RowOrder,
}

// A bitset occupies the same storage alternative as insertion-order links;
// neither the ABI prefix nor dense list storage changes.
enum RowOrder {
    Dense,
    Linked(Order),
    Bits64(u64),
    Bits128([u64; 2]),
    Bits256([u64; 4]),
}
impl RowOrder {
    fn from_bits(bits: enum_bits::Bits) -> Self {
        match bits {
            enum_bits::Bits::Small(word) => Self::Bits64(word),
            enum_bits::Bits::Medium(words) => Self::Bits128(words),
            enum_bits::Bits::Large(words) => Self::Bits256(words),
        }
    }
    fn linked(&self) -> Option<&Order> {
        match self {
            Self::Linked(order) => Some(order),
            _ => None,
        }
    }
    fn linked_mut(&mut self) -> Option<&mut Order> {
        match self {
            Self::Linked(order) => Some(order),
            _ => None,
        }
    }
    fn is_none(&self) -> bool {
        matches!(self, Self::Dense)
    }
    fn is_some(&self) -> bool {
        !self.is_none()
    }
}

const _: () = {
    assert!(std::mem::size_of::<enum_bits::Bits>() <= std::mem::size_of::<Order>());
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
                crate::list_layout::row_bytes(
                    key.kind,
                    key.inline_bytes as usize,
                    key.has_inline_range(),
                )
            } else {
                (key.slot_words() + value.map_or(0, Type::slot_words)) * 16
            },
            removed: false,
            order: RowOrder::Dense,
        }
    }
    pub fn ordered(key: &'static Type, value: Option<&'static Type>) -> Self {
        Self {
            order: RowOrder::Linked(Order::new()),
            // Hash rows retain canonical slots, including an unused value slot
            // for sets; their references and ownership traversal share this ABI.
            stride: (key.slot_words() + value.map_or(1, Type::slot_words)) * 16,
            ..Self::new(key, value)
        }
    }
    pub fn enum_set(key: &'static Type) -> Self {
        debug_assert!(enum_bits::eligible(key));
        Self {
            order: RowOrder::from_bits(enum_bits::Bits::new(enum_bits::cardinality(key))),
            ..Self::new(key, None)
        }
    }
    // Expose the storage alternatives to callers so they can dispatch once.
    #[inline(always)]
    pub fn bits(&self) -> Option<enum_bits::Bits> {
        match self.order {
            RowOrder::Bits64(word) => Some(enum_bits::Bits::Small(word)),
            RowOrder::Bits128(words) => Some(enum_bits::Bits::Medium(words)),
            RowOrder::Bits256(words) => Some(enum_bits::Bits::Large(words)),
            _ => None,
        }
    }
    /// None means ordinary row storage; Some reports membership in a bitset.
    /// The caller supplies a valid value of this collection's key type.
    #[inline(always)]
    pub unsafe fn find_bitset(&self, key: u128) -> Option<Option<usize>> {
        match &self.order {
            RowOrder::Bits64(word) => {
                let index = unsafe { enum_bits::index(self.key, key) };
                Some((word & (1 << index) != 0).then_some(index))
            }
            RowOrder::Bits128(words) => {
                let index = unsafe { enum_bits::index(self.key, key) };
                Some((words[index / 64] & (1 << (index % 64)) != 0).then_some(index))
            }
            RowOrder::Bits256(words) => {
                let index = unsafe { enum_bits::index(self.key, key) };
                Some((words[index / 64] & (1 << (index % 64)) != 0).then_some(index))
            }
            _ => None,
        }
    }
    /// Copy only mask storage. Its validated width and row metadata need no
    /// reclassification, reservation, or canonical-value traversal.
    pub fn copy_bitset(&self) -> Option<Self> {
        Some(Self {
            data: Words::new(),
            removed: false,
            key: self.key,
            value: self.value,
            stride: self.stride,
            order: RowOrder::from_bits(self.bits()?),
        })
    }
    pub fn set_bits(&mut self, bits: enum_bits::Bits) {
        assert!(self.bits().is_some(), "enum set");
        self.order = RowOrder::from_bits(bits);
    }
    pub fn key_ref(&mut self, index: usize) -> *const u128 {
        if let Some(bits) = self.bits() {
            assert!(bits.contains(index));
            enum_bits::reference(self.key, index)
        } else {
            self.slot(index, false)
        }
    }
    pub fn len(&self) -> usize {
        match &self.order {
            RowOrder::Bits64(word) => return word.count_ones() as usize,
            RowOrder::Bits128(words) => {
                return words.iter().map(|word| word.count_ones() as usize).sum();
            }
            RowOrder::Bits256(words) => {
                return words.iter().map(|word| word.count_ones() as usize).sum();
            }
            _ => {}
        }
        self.order.linked().map_or_else(
            || self.data.len() / self.stride - usize::from(self.removed),
            Order::len,
        )
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn capacity(&self) -> usize {
        if self.bits().is_some() {
            return enum_bits::cardinality(self.key);
        }
        let rows = self.data.capacity() / self.stride;
        self.order
            .linked()
            .map_or(rows, |order| rows.min(order.capacity()))
    }
    #[cfg(test)]
    pub fn storage_bytes(&self) -> usize {
        self.data.capacity()
            + self.order.linked().map_or(0, |order| {
                order.capacity() * 2 * std::mem::size_of::<usize>()
            })
    }
    pub fn row_bytes(&self) -> usize {
        self.stride
    }
    pub fn get(&self, index: usize) -> Entry {
        if let Some(bits) = self.bits() {
            assert!(bits.contains(index));
            return Entry {
                key: enum_bits::value(self.key, index),
                value: 0,
            };
        }
        assert!(index < self.data.len() / self.stride);
        debug_assert!(
            self.order
                .linked()
                .map_or(index < self.len(), |order| order.contains(index))
        );
        self.read_row(index)
    }
    /// Read only the key used by a hash-table equality probe. Unlike `get`,
    /// this does not decode an unused dictionary value or return a full row.
    #[inline]
    pub fn key(&self, index: usize) -> u128 {
        if let Some(bits) = self.bits() {
            assert!(bits.contains(index));
            return enum_bits::value(self.key, index);
        }
        assert!(index < self.data.len() / self.stride);
        debug_assert!(
            self.order
                .linked()
                .map_or(index < self.len(), |order| order.contains(index))
        );
        // SAFETY: the bounds and live-row checks are the same as `get`.
        unsafe {
            let base = self.data.as_ptr().add(index * self.stride);
            if self.packed() {
                read_scalar(base, self.key)
            } else {
                base.cast::<u128>().read()
            }
        }
    }

    /// The first word of row `index`'s key, for a key type whose rows hold
    /// whole key words: text keys are never bitset members or packed
    /// scalars. A probe reads candidates through this instead of repeating
    /// `key`'s storage dispatch for each one (issue #931).
    #[inline]
    pub fn key_word(&self, index: usize) -> u64 {
        debug_assert!(self.bits().is_none() && !self.packed());
        // Rows are whole, so a row that starts inside the data ends inside
        // it. This form needs no division, unlike `index < len / stride`.
        debug_assert!(self.data.len().is_multiple_of(self.stride));
        let start = index.checked_mul(self.stride);
        assert!(start.is_some_and(|start| start < self.data.len()));
        // SAFETY: the row lies inside the initialized rows.
        unsafe {
            self.data
                .as_ptr()
                .add(index * self.stride)
                .cast::<u64>()
                .read()
        }
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
        let sparse = self.order.linked().into_iter().flat_map(Order::indices);
        EntryIndices {
            inner: dense
                .chain(sparse)
                .chain(enum_bits::Indices(self.bits().unwrap_or_default())),
            remaining: self.len(),
        }
    }
    pub fn vacant(&self) -> usize {
        self.order
            .linked()
            .map_or_else(|| self.len(), Order::vacant)
    }
    pub fn first(&self) -> Option<usize> {
        if let Some(bits) = self.bits() {
            return enum_bits::Indices(bits).next();
        }
        self.order.linked().expect("hash entries").first()
    }
    pub fn next(&self, index: usize) -> Option<usize> {
        if let Some(bits) = self.bits() {
            return enum_bits::Indices(bits.after(index)).next();
        }
        self.order.linked().expect("hash entries").next(index)
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
        self.slot_address(index, value)
    }
    /// [`Self::slot`] through a shared reference: lookups that lend an element
    /// (`get_ref`, `get_mut`) must not create a `&mut Entries`, since a shared
    /// loan of the collection may be live on other threads (issue #869).
    pub fn slot_address(&self, index: usize, value: bool) -> *mut u128 {
        assert!(
            self.bits().is_none(),
            "bitset members have no writable slots"
        );
        assert!(index < self.data.len() / self.stride);
        debug_assert!(
            self.order
                .linked()
                .map_or(index < self.len(), |order| order.contains(index))
        );
        unsafe {
            self.data
                .base()
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
        let sparse = self.order.linked().into_iter().flat_map(Order::indices);
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
        if self.bits().is_some() {
            return Ok(());
        }
        if let Some(order) = self.order.linked_mut() {
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
        match &mut self.order {
            RowOrder::Bits64(word) => {
                *word |= 1 << unsafe { enum_bits::index(self.key, entry.key) };
                return;
            }
            RowOrder::Bits128(words) => {
                let index = unsafe { enum_bits::index(self.key, entry.key) };
                words[index / 64] |= 1 << (index % 64);
                return;
            }
            RowOrder::Bits256(words) => {
                let index = unsafe { enum_bits::index(self.key, entry.key) };
                words[index / 64] |= 1 << (index % 64);
                return;
            }
            _ => {}
        }
        assert!(self.len() < self.capacity());
        let index = self.vacant();
        if let Some(order) = self.order.linked_mut() {
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
    /// Insert a dense row at `index`, at most `len()`, moving the later rows
    /// up one row with a single `memmove`. Reservation belongs to the
    /// surrounding transactional operation.
    pub unsafe fn insert(&mut self, index: usize, entry: Entry) {
        assert!(self.order.is_none(), "only dense lists insert");
        let len = self.len();
        assert!(index <= len && len < self.capacity());
        // Extraction scratch is dead at this mutation, as for `push`.
        self.data.truncate(len * self.stride);
        self.removed = false;
        self.data.open_gap(index * self.stride, self.stride);
        self.relocate(index + 1);
        unsafe {
            self.set(index, false, entry.key);
        }
    }
    /// Append byte copies of `count` dense rows of `source`, starting at row
    /// `first`, in one copy. Both sides are dense lists of one element type
    /// whose elements own nothing; the caller reserved the capacity.
    pub fn extend_rows(&mut self, source: &Self, first: usize, count: usize) {
        assert!(self.order.is_none() && source.order.is_none() && self.stride == source.stride);
        assert!(first + count <= source.len() && self.len() + count <= self.capacity());
        self.data.truncate(self.len() * self.stride);
        self.removed = false;
        self.data
            .extend_from_slice(&source.data[first * self.stride..(first + count) * self.stride]);
    }
    pub fn clear(&mut self) {
        match &mut self.order {
            RowOrder::Bits64(word) => *word = 0,
            RowOrder::Bits128(words) => *words = [0; 2],
            RowOrder::Bits256(words) => *words = [0; 4],
            _ => {}
        }
        self.data.clear();
        self.removed = false;
        if let Some(order) = self.order.linked_mut() {
            order.clear();
        }
    }
    pub fn drain(&mut self) -> impl Iterator<Item = Entry> + '_ {
        let bits = self.bits().unwrap_or_default();
        if self.bits().is_some() {
            self.set_bits(bits.empty());
        }
        let count = self.len();
        let base = self.data.as_ptr();
        let stride = self.stride;
        let key = self.key;
        let value = self.value;
        let packed = self.packed();
        self.data.clear();
        self.removed = false;
        let dense = 0..if self.order.is_none() { count } else { 0 };
        let sparse = self
            .order
            .linked_mut()
            .map(Order::drain)
            .into_iter()
            .flatten();
        // The reserved buffer stays live and its trivially copied words remain
        // initialized. The returned iterator borrows self, preventing reallocation.
        dense
            .chain(sparse)
            .map(move |i| unsafe { read_row(base.add(i * stride), key, value, packed) })
            .chain(enum_bits::Indices(bits).map(move |i| Entry {
                key: enum_bits::value(key, i),
                value: 0,
            }))
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
        match &mut self.order {
            RowOrder::Bits64(word) => {
                *word &= !(1 << index);
                return entry;
            }
            RowOrder::Bits128(words) => {
                words[index / 64] &= !(1 << (index % 64));
                return entry;
            }
            RowOrder::Bits256(words) => {
                words[index / 64] &= !(1 << (index % 64));
                return entry;
            }
            _ => {}
        }
        if let Some(order) = self.order.linked_mut() {
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
            unsafe { relocation.repair(base.add(row * self.stride).cast(), self.key) }
        }
    }
    fn exchange_bytes(&mut self, i: usize, j: usize) {
        let (i, j) = if i < j { (i, j) } else { (j, i) };
        let (left, right) = self.data.split_at_mut(j * self.stride);
        left[i * self.stride..(i + 1) * self.stride].swap_with_slice(&mut right[..self.stride]);
    }
    /// This dense list's live rows, for the sorts in [`sort`].
    pub(crate) fn dense_rows(&mut self) -> sort::Rows<'_> {
        assert!(self.order.is_none(), "only dense lists sort");
        let len = self.len();
        let packed = self.packed();
        sort::Rows::new(
            &mut self.data[..len * self.stride],
            self.stride,
            self.key,
            packed,
        )
    }
    /// Sort dense rows in place without allocating, as [`sort::Rows::sort_unstable_by`].
    #[cfg(test)]
    pub fn sort_unstable_by(&mut self, compare: impl FnMut(u128, u128) -> Ordering) {
        self.dense_rows().sort_unstable_by(compare);
    }
    /// Sort dense rows of any layout, as [`sort::Rows::sort_unstable_rows_by`].
    pub fn sort_unstable_rows_by(&mut self, compare: impl FnMut(u128, u128) -> Ordering) {
        self.dense_rows().sort_unstable_rows_by(compare);
    }
    /// Sort dense rows stably, as [`sort::Rows::sort_stable_rows_by`].
    pub fn sort_stable_rows_by(
        &mut self,
        compare: impl FnMut(u128, u128) -> Ordering,
    ) -> Result<(), memory::AllocError> {
        self.dense_rows().sort_stable_rows_by(compare)
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
/// Exchange two distinct or identical dense rows of `key` in place, as
/// [`Entries::swap`] does, for a slice view's rows (issue #729).
///
/// # Safety
/// `a` and `b` address live rows of `key`'s dense layout, `stride` bytes
/// each, in one list's storage, which the caller holds exclusively.
pub(crate) unsafe fn exchange_rows(a: *mut u8, b: *mut u8, stride: usize, key: &Type) {
    if a == b {
        return;
    }
    let relocation = Relocation::of(key);
    // SAFETY: distinct rows of one dense layout do not overlap.
    unsafe {
        std::ptr::swap_nonoverlapping(a, b, stride);
        for row in [a, b] {
            relocation.repair(row.cast(), key);
        }
    }
}

impl Relocation {
    /// Repair the inline address of the row moved into `slot`.
    unsafe fn repair(&self, slot: *mut u128, key: &Type) {
        // SAFETY: the caller passes a live row of `key`.
        unsafe {
            match self {
                Relocation::None => {}
                Relocation::Flat => ranges::relocate_flat(slot),
                Relocation::General => ranges::relocate(slot, key),
            }
        }
    }
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

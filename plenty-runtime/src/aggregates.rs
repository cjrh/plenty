//! Rust-owned metadata and buffers behind a small, layout-stable native prefix.
use crate::entries::{Entries, Entry};
use crate::generators::{plenty_generator_resume, Generator};
use crate::memory::{self, plenty_release, plenty_retain, AllocError, Header};
use crate::ranges::{self, Range};
use crate::strings::{self, Text};
use std::io::Write;

pub(crate) mod executor_map;
pub(crate) mod executor_reduce;

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
}
const _: () = {
    assert!(size_of::<Type>() == 56);
    assert!(std::mem::offset_of!(Type, key) == 8);
    assert!(std::mem::offset_of!(Type, name) == 24);
    assert!(std::mem::offset_of!(Type, variants) == 40);
    assert!(size_of::<Variant>() == 32);
};
impl Type {
    fn managed(&self) -> bool {
        self.kind == b'B'
            || matches!(
                self.kind,
                b's' | b'L' | b'S' | b'D' | b'E' | b'C' | b'G' | b'F' | b'X' | b'Y' | b'P' | b'Z'
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
    fn payload(&self, value: u128) -> Option<&Type> {
        self.variants[self.tag(value)].fields.first().copied()
    }
    fn tag_bits(&self) -> u32 {
        self.variants.len().next_power_of_two().ilog2().max(1)
    }
    pub(crate) fn unpack(&self, value: u128) -> u128 {
        (value & u64::MAX as u128) | ((value >> (64 + self.tag_bits())) << 64)
    }
    fn pack(&self, value: u128, tag: usize) -> u128 {
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
}
#[repr(C)]
struct Record {
    header: Header,
    ty: *const Type,
    tag_or_hook: u64,
    fields: [u128; 0],
}
const _: () = assert!(std::mem::offset_of!(Collection, ty) == 16);
const _: () = assert!(std::mem::offset_of!(Record, fields) == 32);

unsafe fn value_type<'a>(value: u128) -> &'a Type {
    // SAFETY: all managed aggregates have an immutable static Type pointer at byte 16.
    unsafe {
        &**(value as *const u8)
            .add(size_of::<Header>())
            .cast::<*const Type>()
    }
}
pub(crate) unsafe fn retain(value: u128, ty: &Type) {
    if ty.kind == b'H' {
        unsafe {
            crate::closures::retain(value as *mut u128);
        }
    } else if ty.kind == b'B' {
        if let Some(t) = ty.payload(value) {
            unsafe {
                retain(ty.unpack(value), t);
            }
        }
    } else if ty.managed() {
        unsafe {
            plenty_retain(value as *mut Header);
        }
    }
}
pub(crate) unsafe fn release(value: u128, ty: &Type) {
    if ty.kind == b'H' {
        unsafe {
            crate::closures::release(value as *mut u128);
        }
    } else if ty.kind == b'B' {
        if let Some(t) = ty.payload(value) {
            unsafe {
                release(ty.unpack(value), t);
            }
        }
    } else if ty.kind == b'G' && ty.inline_bytes != 0 {
        unsafe { crate::generators::release_inline(value as *mut Generator) };
    } else if ty.managed() {
        unsafe {
            plenty_release(value as *mut Header);
        }
    }
}

fn collection_new(ty: &'static Type) -> *mut Collection {
    try_collection_new(ty, 0).unwrap_or_else(|_| crate::fail("collection allocation failed"))
}
fn try_collection_new(ty: &'static Type, capacity: usize) -> Result<*mut Collection, AllocError> {
    let mut collection = Collection {
        header: Header::new(collection_destroy),
        ty,
        entries: Entries::new(ty.key(), ty.value),
        table: Vec::new(),
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
        // Queue in reverse so complete child destruction follows index order.
        for entry in (*c).entries.iter().rev() {
            if let Some(value) = &ty.value {
                release(entry.value, value);
            }
            release(entry.key, ty.key());
        }
        std::ptr::drop_in_place(c);
        memory::free::<Collection, u8>(c, 0);
    }
}
fn record_count(ty: &Type, tag: u64) -> usize {
    if ty.kind == b'C' {
        ty.variants.len()
    } else {
        ty.variants
            .get(tag as usize)
            .unwrap_or_else(|| crate::fail("invalid enum variant"))
            .fields
            .len()
    }
}
fn field_type(ty: &Type, tag: u64, index: usize) -> &Type {
    if ty.kind == b'C' {
        ty.variants[index].fields[0]
    } else {
        ty.variants[tag as usize].fields[index]
    }
}
fn record_words(ty: &Type, tag: u64) -> usize {
    (0..record_count(ty, tag))
        .map(|i| field_type(ty, tag, i).slot_words())
        .sum()
}
unsafe fn record_slot(record: *mut Record, index: usize) -> *mut u128 {
    unsafe {
        let ty = &*(*record).ty;
        let offset: usize = (0..index)
            .map(|i| field_type(ty, (*record).tag_or_hook, i).slot_words())
            .sum();
        std::ptr::addr_of_mut!((*record).fields)
            .cast::<u128>()
            .add(offset)
    }
}
fn record_new(ty: &'static Type, tag_or_hook: u64) -> *mut Record {
    try_record_new(ty, tag_or_hook).unwrap_or_else(|_| crate::fail("record allocation failed"))
}
fn try_record_new(ty: &'static Type, tag_or_hook: u64) -> Result<*mut Record, AllocError> {
    let count = record_words(ty, tag_or_hook);
    let r = memory::try_allocate::<Record, u128>(count)?;
    unsafe {
        r.write(Record {
            header: Header::new(record_destroy),
            ty,
            tag_or_hook,
            fields: [],
        });
    }
    Ok(r)
}

/// Build the endpoint pair transactionally. The zeroed pair can be released
/// before either endpoint is initialized if the bounded queue cannot allocate.
pub(crate) unsafe fn channel_new(result: &'static Type, capacity: usize) -> u128 {
    if capacity == 0 {
        return wrap(wrap(0, 0), 1); // Err(ChannelError.InvalidCapacity)
    }
    unsafe {
        let pair = result.variants[0].fields[0];
        let build = || -> Result<u128, AllocError> {
            let record = try_record_new(pair, 0)?;
            match crate::channels::create(pair.variants[0].fields[0].key(), capacity) {
                Ok((sender, receiver)) => {
                    record_slot(record, 0).write(sender);
                    record_slot(record, 1).write(receiver);
                    Ok(record as u128)
                }
                Err(error) => {
                    plenty_release(record.cast());
                    Err(error)
                }
            }
        };
        match build() {
            Ok(pair) => wrap(pair, 0),
            Err(error) => wrap(wrap(wrap(0, error as u64), 1), 1),
        }
    }
}
unsafe extern "C" fn record_destroy(header: *mut Header) {
    // No Rust reference to the record survives a user callback. The callback
    // may read or mutate fields through its exclusive native receiver slot.
    unsafe {
        let r = header.cast::<Record>();
        let ty = &*(*r).ty;
        let tag = (*r).tag_or_hook;
        let count = record_count(ty, tag);
        if ty.kind == b'C' && tag != 0 {
            (*header)
                .refs
                .store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
            // Reconstitute an exposed native pointer before casting to its ABI
            // function type; transmuting an integer directly loses provenance.
            let address = std::ptr::with_exposed_provenance::<()>(tag as usize);
            let hook: unsafe extern "C" fn(*mut u128) = std::mem::transmute(address);
            let mut owner = r as u128;
            memory::with_nested_drops(|| hook(&mut owner));
            if owner != r as u128 {
                crate::fail("destructor replaced its receiver");
            }
        }
        for i in (0..count).rev() {
            release(*record_slot(r, i), field_type(ty, tag, i));
        }
        memory::free::<Record, u128>(r, record_words(ty, tag));
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
        for &b in unsafe { strings::bytes(value as *const Text) } {
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
    /// Reserve both buffers before publishing a new hash index. On error,
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
            for (i, entry) in self.entries.iter().enumerate() {
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
    /// occurs until both entry and bucket capacity are available.
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
                    let old = self.entries.get(index).value;
                    self.entries.set(index, true, entry.value);
                    release(entry.key, ty.key());
                    if let Some(value_type) = ty.value {
                        release(old, value_type);
                    }
                } else {
                    let bucket = self.bucket(entry.key);
                    self.table[bucket] = self.entries.len() + 1;
                    self.entries.push(entry);
                }
            }
            source.entries.clear();
            Ok(())
        }
    }

    unsafe fn try_insert(&mut self, key: u128, value: u128) -> Result<(), AllocError> {
        unsafe {
            if self.ty().kind != b'L' {
                if let Some(i) = self.find(key) {
                    if let Some(ty) = &self.ty().value {
                        retain(value, ty);
                        release(self.entries.get(i).value, ty);
                    }
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
            retain(key, self.ty().key());
            if let Some(ty) = &self.ty().value {
                retain(value, ty);
            }
            if self.ty().kind != b'L' {
                let bucket = self.bucket(key);
                self.table[bucket] = self.entries.len() + 1;
            }
            self.entries.push(Entry { key, value });
            Ok(())
        }
    }
    /// Preserve insertion order and rebuild bucket indices in existing storage.
    /// Dictionary payload owners are transferred, never retained or released.
    /// Sets have a zero payload; Some(0) still distinguishes removal from a miss.
    unsafe fn remove(&mut self, key: u128) -> Option<u128> {
        unsafe {
            let index = self.find(key)?;
            let entry = self.entries.remove(index);
            self.table.fill(0);
            for (i, entry) in self.entries.iter().enumerate() {
                let mut bucket = hash(entry.key, self.ty().key()) as usize & (self.table.len() - 1);
                while self.table[bucket] != 0 {
                    bucket = (bucket + 1) & (self.table.len() - 1);
                }
                self.table[bucket] = i + 1;
            }
            release(entry.key, self.ty().key());
            Some(entry.value)
        }
    }

    unsafe fn at(&self, index: usize) -> u128 {
        unsafe {
            let value = self.entries.get(index).key;
            retain(value, self.ty().key());
            value
        }
    }
}

unsafe fn equal(a: u128, b: u128, ty: &Type) -> bool {
    // Immutable payloads can form shared DAGs. Memoize aggregate pairs so IEEE
    // comparisons do not turn a shared float-containing graph into exponential work.
    unsafe { equal_inner(a, b, ty, &mut [(0, 0); 256], &mut 0) }
}
unsafe fn equal_inner(
    a: u128,
    b: u128,
    ty: &Type,
    seen: &mut [(u128, u128); 256],
    cursor: &mut usize,
) -> bool {
    unsafe {
        if a == b && ty.reflexive {
            return true;
        }
        if matches!(ty.kind, b'C' | b'E' | b'L' | b'D') {
            if seen.contains(&(a, b)) {
                return true;
            }
            seen[*cursor] = (a, b);
            *cursor = (*cursor + 1) % seen.len();
        }
        match ty.kind {
            b'B' => {
                if ty.tag(a) != ty.tag(b) {
                    return false;
                }
                ty.payload(a)
                    .is_none_or(|t| equal_inner(ty.unpack(a), ty.unpack(b), t, seen, cursor))
            }
            b'f' => f32::from_bits(a as u32) == f32::from_bits(b as u32),
            b'd' => f64::from_bits(a as u64) == f64::from_bits(b as u64),
            b's' => strings::bytes(a as *const Text) == strings::bytes(b as *const Text),
            b'C' | b'E' => {
                let (a, b) = (a as *const Record, b as *const Record);
                if (*(*a).ty).name != (*(*b).ty).name {
                    return false;
                }
                if ty.kind == b'E' && (*a).tag_or_hook != (*b).tag_or_hook {
                    return false;
                }
                (0..record_count(ty, (*a).tag_or_hook)).all(|i| {
                    equal_inner(
                        *record_slot(a.cast_mut(), i),
                        *record_slot(b.cast_mut(), i),
                        field_type(ty, (*a).tag_or_hook, i),
                        seen,
                        cursor,
                    )
                })
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
                        if !equal_inner(entry.key, b.entries.get(i).key, ty.key(), seen, cursor) {
                            return false;
                        }
                    } else {
                        let Some(j) = b.find(entry.key) else {
                            return false;
                        };
                        if ty.kind == b'D'
                            && !equal_inner(
                                entry.value,
                                b.entries.get(j).value,
                                ty.value(),
                                seen,
                                cursor,
                            )
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
unsafe fn copy(value: u128, ty: &Type) -> u128 {
    unsafe { try_copy(value, ty) }.unwrap_or_else(|_| crate::fail("copy allocation failed"))
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
            if strings::utf8(line.value as *const Text).is_empty() {
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
            crate::files::write(file, strings::utf8(entry.key as *const Text))?;
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
        let separator = strings::utf8(separator);
        if separator.is_empty() {
            crate::fail("string split requires a nonempty separator");
        }
        let pieces = strings::utf8(text).split(separator);
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
        for index in 0..source.entries.len() {
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

unsafe fn try_copy(value: u128, ty: &Type) -> Result<u128, AllocError> {
    unsafe {
        if !ty.affine {
            retain(value, ty);
            return Ok(value);
        }
        match ty.kind {
            b'B' => match ty.payload(value) {
                Some(t) => Ok(ty.pack(try_copy(ty.unpack(value), t)?, ty.tag(value))),
                None => Ok(value),
            },
            b'C' | b'E' => {
                let source = value as *const Record;
                if ty.kind == b'C' && (*source).tag_or_hook != 0 {
                    crate::fail("cannot copy a class with custom cleanup");
                }
                let tag = (*source).tag_or_hook;
                let count = record_count(ty, tag);
                let result = try_record_new(&*(*source).ty, tag)?;
                for i in 0..count {
                    match try_copy(*record_slot(source.cast_mut(), i), field_type(ty, tag, i)) {
                        Ok(field) => {
                            ranges::store(record_slot(result, i), field, field_type(ty, tag, i))
                        }
                        Err(error) => {
                            // Only the initialized prefix owns values. Never
                            // run a whole-record destructor on a partial copy.
                            for j in (0..i).rev() {
                                release(*record_slot(result, j), field_type(ty, tag, j));
                            }
                            memory::free::<Record, u128>(result, record_words(ty, tag));
                            return Err(error);
                        }
                    }
                }
                Ok(result as u128)
            }
            b'L' | b'S' | b'D' => {
                let source = &*(value as *const Collection);
                let result = try_collection_new(&*source.ty, source.entries.len())?;
                let owner = OwnedValue {
                    value: result as u128,
                    ty,
                };
                for entry in source.entries.iter() {
                    let key = OwnedValue {
                        value: try_copy(entry.key, ty.key())?,
                        ty: ty.key(),
                    };
                    let value = match ty.value {
                        Some(t) => Some(OwnedValue {
                            value: try_copy(entry.value, t)?,
                            ty: t,
                        }),
                        None => None,
                    };
                    (*result).try_insert(key.value, value.as_ref().map_or(0, |v| v.value))?;
                }
                Ok(owner.into_value())
            }
            _ => {
                retain(value, ty);
                Ok(value)
            }
        }
    }
}

unsafe fn render(value: u128, ty: &Type, out: &mut crate::render_buffer::Buffer) {
    if out.failed() {
        return;
    }
    unsafe {
        match ty.kind {
            b'B' => {
                out.extend_from_slice(ty.name.as_bytes());
                out.push(b'.');
                let variant = &ty.variants[ty.tag(value)];
                out.extend_from_slice(variant.name.as_bytes());
                if let Some(t) = ty.payload(value) {
                    out.push(b'(');
                    render(ty.unpack(value), t, out);
                    out.push(b')');
                }
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
            b'b' => out.extend_from_slice(if value == 0 { b"False" } else { b"True" }),
            b's' => crate::io::repr(value as *const Text, false, out),
            b'F' => out.extend_from_slice(
                if crate::files::closed(value as *const crate::files::File) {
                    b"File(closed)"
                } else {
                    b"File(open)"
                },
            ),
            b'C' | b'E' => {
                let r = value as *const Record;
                let tuple = ty.kind == b'E' && ty.name.starts_with("tuple[");
                if !tuple {
                    out.extend_from_slice(ty.name.as_bytes());
                }
                let count = record_count(ty, (*r).tag_or_hook);
                if ty.kind == b'E' && !tuple {
                    out.push(b'.');
                    out.extend_from_slice(ty.variants[(*r).tag_or_hook as usize].name.as_bytes());
                }
                if count > 0 || ty.kind == b'C' {
                    out.push(b'(');
                    for i in 0..count {
                        if out.failed() {
                            break;
                        }
                        if i != 0 {
                            out.extend_from_slice(b", ");
                        }
                        if ty.kind == b'C' {
                            out.extend_from_slice(ty.variants[i].name.as_bytes());
                            out.push(b'=');
                        }
                        render(
                            *record_slot(r.cast_mut(), i),
                            field_type(ty, (*r).tag_or_hook, i),
                            out,
                        );
                    }
                    if tuple && count == 1 {
                        out.push(b',');
                    }
                    out.push(b')');
                }
            }
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
        if op == 14 || op == 33 {
            let ty = if descriptor.is_null() {
                value_type(a)
            } else {
                &*descriptor
            };
            return if op == 14 {
                copy(a, ty)
            } else {
                match try_copy(a, ty) {
                    Ok(value) => wrap(value, 0),
                    Err(error) => wrap(wrap(0, error as u64), 1),
                }
            };
        }
        if !descriptor.is_null() && (*descriptor).kind == b's' && !matches!(op, 110 | 111) {
            let text = a as *const Text;
            return match op {
                5 => (*text).scalar_len as u128,
                12 => (*text).byte_len as u128,
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
            114 => match strings::try_get(a as *const Text, b as i64) {
                Ok(Some(text)) => wrap(text as u128, 0),
                Ok(None) => crate::fail("index out of bounds"),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            115 | 116 => {
                let source = strings::utf8(a as *const Text);
                let tail = &source[b as usize..];
                let width = tail.chars().next().expect("valid string cursor").len_utf8();
                if op == 116 {
                    b + width as u128
                } else {
                    match strings::try_new(&tail[..width]) {
                        Ok(text) => wrap(text as u128, 0),
                        Err(error) => wrap(wrap(0, error as u64), 1),
                    }
                }
            }
            110 | 111 => {
                #[cfg(feature = "allocation-checks")]
                if op == 111
                    && (*descriptor).kind == b's'
                    && strings::bytes(a as *const Text).starts_with(b"__test_")
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
                            .write_all(strings::bytes(a as *const Text))
                            .and_then(|()| stdout.write_all(b"\n"))
                            .map(|()| 0)
                            .map_err(crate::text_io::Error::from),
                    );
                }
                let mut out = crate::render_buffer::Buffer::default();
                if op == 111 && (*descriptor).kind == b's' {
                    out.extend_from_slice(strings::bytes(a as *const Text));
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
                strings::utf8(a as *const Text),
                strings::utf8(b as *const Text),
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
                strings::utf8(b as *const Text),
            )),
            94 | 95 => {
                crate::text_io::result(crate::files::flush(a as *mut crate::files::File, op == 95))
            }
            87 | 88 => crate::text_io::result(crate::text_io::write_text(
                strings::utf8(a as *const Text),
                strings::utf8(b as *const Text),
                op == 88,
            )),
            86 => {
                crate::text_io::result(crate::text_io::read_text(strings::utf8(a as *const Text)))
            }
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
            78 => crate::numbers::parse(strings::utf8(a as *const Text), (*descriptor).kind),
            76 | 77 => {
                let source = strings::utf8(a as *const Text);
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
                let source = strings::utf8(a as *const Text);
                let affix = strings::utf8(b as *const Text);
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
                for (index, entry) in target.entries.iter().enumerate() {
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
            52 => strings::utf8(a as *const Text)
                .matches(strings::utf8(b as *const Text))
                .count() as u128,
            50 | 51 => {
                let source = strings::utf8(a as *const Text);
                let needle = strings::utf8(b as *const Text);
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
                strings::utf8(a as *const Text).starts_with(strings::utf8(b as *const Text)) as u128
            }
            49 => {
                strings::utf8(a as *const Text).ends_with(strings::utf8(b as *const Text)) as u128
            }
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
            37 => match strings::try_get(a as *const Text, b as i64) {
                Ok(Some(text)) => wrap(wrap(text as u128, 1), 0),
                Ok(None) => wrap(wrap(0, 0), 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            107 => {
                let ty = (*descriptor).variants[0].fields[0];
                let pieces = crate::text_lines::Lines::new(strings::utf8(a as *const Text), b != 0);
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
            108 => match try_record_new(&*descriptor, a as u64) {
                Ok(record) => wrap(record as u128, 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
            109 => {
                (*(a as *mut Record)).tag_or_hook = b as u64;
                0
            }
            20 | 30 => record_new(&*descriptor, a as u64) as u128,
            21 => {
                let r = a as *mut Record;
                let ty = &*(*r).ty;
                let i = index(b as i64, record_count(ty, (*r).tag_or_hook));
                let field_ty = field_type(ty, (*r).tag_or_hook, i);
                let slot = record_slot(r, i);
                retain(value, field_ty);
                release(*slot, field_ty);
                ranges::store(slot, value, field_ty);
                0
            }
            22 => (*(a as *const Record)).tag_or_hook as u128,
            23 | 25 | 31 => {
                let r = a as *mut Record;
                let ty = &*(*r).ty;
                if op != 31 && (*r).tag_or_hook as u128 != value {
                    crate::fail("invalid enum projection");
                }
                let i = index(b as i64, record_count(ty, (*r).tag_or_hook));
                let slot = record_slot(r, i);
                let field = *slot;
                if op == 25 {
                    *slot = 0;
                } else {
                    retain(field, field_type(ty, (*r).tag_or_hook, i));
                }
                field
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

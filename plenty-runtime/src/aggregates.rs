//! Rust-owned metadata and buffers behind a small, layout-stable native prefix.
use crate::generators::{plenty_generator_resume, Generator};
use crate::memory::{self, plenty_release, plenty_retain, AllocError, Header};
use crate::strings::{self, Text};
use std::io::Write;

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
                b's' | b'L' | b'S' | b'D' | b'R' | b'E' | b'C' | b'G'
            )
    }
    fn key(&self) -> &Type {
        self.key.expect("collection key type")
    }
    fn value(&self) -> &Type {
        self.value.expect("dictionary value type")
    }
    fn payload(&self, value: u128) -> Option<&Type> {
        self.variants[((value >> 64) & 1) as usize]
            .fields
            .first()
            .copied()
    }
}
pub(crate) fn payload(value: u128) -> u128 {
    (value & u64::MAX as u128) | ((value >> 65) << 64)
}
pub(crate) fn wrap(value: u128, tag: u64) -> u128 {
    (value & u64::MAX as u128) | (((value >> 64) * 2 + tag as u128) << 64)
}

#[derive(Clone, Copy)]
struct Entry {
    key: u128,
    value: u128,
}
#[repr(C)]
struct Collection {
    header: Header,
    ty: *const Type,
    entries: Vec<Entry>,
    table: Vec<usize>,
    start: i64,
    stop: i64,
    step: i64,
    range_len: usize,
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
    // SAFETY: all managed aggregates have a raw Rc-owned Type pointer at byte 16.
    unsafe {
        &**(value as *const u8)
            .add(size_of::<Header>())
            .cast::<*const Type>()
    }
}
pub(crate) unsafe fn retain(value: u128, ty: &Type) {
    if ty.kind == b'B' {
        if let Some(t) = ty.payload(value) {
            unsafe {
                retain(payload(value), t);
            }
        }
    } else if ty.managed() {
        unsafe {
            plenty_retain(value as *mut Header);
        }
    }
}
pub(crate) unsafe fn release(value: u128, ty: &Type) {
    if ty.kind == b'B' {
        if let Some(t) = ty.payload(value) {
            unsafe {
                release(payload(value), t);
            }
        }
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
        entries: Vec::new(),
        table: Vec::new(),
        start: 0,
        stop: 0,
        step: 0,
        range_len: 0,
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
fn record_new(ty: &'static Type, tag_or_hook: u64) -> *mut Record {
    try_record_new(ty, tag_or_hook).unwrap_or_else(|_| crate::fail("record allocation failed"))
}
fn try_record_new(ty: &'static Type, tag_or_hook: u64) -> Result<*mut Record, AllocError> {
    let count = record_count(ty, tag_or_hook);
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
unsafe extern "C" fn record_destroy(header: *mut Header) {
    // No Rust reference to the record survives a user callback. The callback
    // may read or mutate fields through its exclusive native receiver slot.
    unsafe {
        let r = header.cast::<Record>();
        let ty = &*(*r).ty;
        let tag = (*r).tag_or_hook;
        let count = record_count(ty, tag);
        if ty.kind == b'C' && tag != 0 {
            (*header).refs = u64::MAX;
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
            release(
                *std::ptr::addr_of!((*r).fields).cast::<u128>().add(i),
                field_type(ty, tag, i),
            );
        }
        memory::free::<Record, u128>(r, count);
    }
}

pub(crate) fn index(index: i64, len: usize) -> usize {
    checked_index(index, len).unwrap_or_else(|| crate::fail("index out of bounds"))
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
        if self.ty().kind == b'R' {
            self.range_len
        } else {
            self.entries.len()
        }
    }
    unsafe fn bucket(&self, key: u128) -> usize {
        unsafe {
            let mut bucket = hash(key, self.ty().key()) as usize & (self.table.len() - 1);
            while self.table[bucket] != 0
                && !equal(
                    self.entries[self.table[bucket] - 1].key,
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
        memory::try_reserve(&mut self.entries, additional)?;
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

    unsafe fn try_insert(&mut self, key: u128, value: u128) -> Result<(), AllocError> {
        unsafe {
            if self.ty().kind != b'L' {
                if let Some(i) = self.find(key) {
                    if let Some(ty) = &self.ty().value {
                        retain(value, ty);
                        release(self.entries[i].value, ty);
                    }
                    self.entries[i].value = value;
                    return Ok(());
                }
            }
            let additional = if self.entries.len() == self.entries.capacity() {
                // Geometric growth without making explicit reserve speculative.
                self.entries
                    .len()
                    .max(4)
                    .min(
                        (isize::MAX as usize / size_of::<Entry>())
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
    unsafe fn at(&self, index: usize) -> u128 {
        if self.ty().kind == b'R' {
            (self.start as i128 + index as i128 * self.step as i128) as u128
        } else {
            unsafe {
                let value = self.entries[index].key;
                retain(value, self.ty().key());
                value
            }
        }
    }
}

unsafe fn equal(a: u128, b: u128, ty: &Type) -> bool {
    // Immutable payloads can form shared DAGs. Memoize aggregate pairs so IEEE
    // comparisons do not turn a shared float-containing graph into exponential work.
    unsafe { equal_inner(a, b, ty, &mut std::collections::HashSet::new()) }
}
unsafe fn equal_inner(
    a: u128,
    b: u128,
    ty: &Type,
    seen: &mut std::collections::HashSet<(u128, u128)>,
) -> bool {
    unsafe {
        if a == b && ty.reflexive {
            return true;
        }
        if matches!(ty.kind, b'C' | b'E' | b'L' | b'D') && !seen.insert((a, b)) {
            return true;
        }
        match ty.kind {
            b'B' => {
                if (a >> 64) & 1 != (b >> 64) & 1 {
                    return false;
                }
                ty.payload(a)
                    .is_none_or(|t| equal_inner(payload(a), payload(b), t, seen))
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
                        *std::ptr::addr_of!((*a).fields).cast::<u128>().add(i),
                        *std::ptr::addr_of!((*b).fields).cast::<u128>().add(i),
                        field_type(ty, (*a).tag_or_hook, i),
                        seen,
                    )
                })
            }
            b'L' | b'S' | b'D' | b'R' => {
                let (a, b) = (&*(a as *const Collection), &*(b as *const Collection));
                if a.len() != b.len() {
                    return false;
                }
                if ty.kind == b'R' {
                    return a.len() == 0
                        || (a.start == b.start && (a.len() == 1 || a.step == b.step));
                }
                for (i, entry) in a.entries.iter().enumerate() {
                    if ty.kind == b'L' {
                        if !equal_inner(entry.key, b.entries[i].key, ty.key(), seen) {
                            return false;
                        }
                    } else {
                        let Some(j) = b.find(entry.key) else {
                            return false;
                        };
                        if ty.kind == b'D'
                            && !equal_inner(entry.value, b.entries[j].value, ty.value(), seen)
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

unsafe fn try_copy(value: u128, ty: &Type) -> Result<u128, AllocError> {
    unsafe {
        if !ty.affine {
            retain(value, ty);
            return Ok(value);
        }
        match ty.kind {
            b'B' => match ty.payload(value) {
                Some(t) => Ok(wrap(
                    try_copy(payload(value), t)?,
                    ((value >> 64) & 1) as u64,
                )),
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
                let fields = std::ptr::addr_of_mut!((*result).fields).cast::<u128>();
                for i in 0..count {
                    match try_copy(
                        *std::ptr::addr_of!((*source).fields).cast::<u128>().add(i),
                        field_type(ty, tag, i),
                    ) {
                        Ok(field) => *fields.add(i) = field,
                        Err(error) => {
                            // Only the initialized prefix owns values. Never
                            // run a whole-record destructor on a partial copy.
                            for j in (0..i).rev() {
                                release(*fields.add(j), field_type(ty, tag, j));
                            }
                            memory::free::<Record, u128>(result, count);
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
                for entry in &source.entries {
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

unsafe fn render(value: u128, ty: &Type, out: &mut Vec<u8>) {
    unsafe {
        match ty.kind {
            b'B' => {
                out.extend_from_slice(ty.name.as_bytes());
                out.push(b'.');
                let variant = &ty.variants[((value >> 64) & 1) as usize];
                out.extend_from_slice(variant.name.as_bytes());
                if let Some(t) = ty.payload(value) {
                    out.push(b'(');
                    render(payload(value), t, out);
                    out.push(b')');
                }
            }
            b'1'..=b'4' => write!(out, "{}", value as i64).unwrap(),
            b'5'..=b'8' => write!(out, "{value}").unwrap(),
            b'f' => write!(out, "{:?}", f32::from_bits(value as u32)).unwrap(),
            b'd' => write!(out, "{:?}", f64::from_bits(value as u64)).unwrap(),
            b'v' => out.extend_from_slice(b"()"),
            b'b' => out.extend_from_slice(if value == 0 { b"False" } else { b"True" }),
            b's' => crate::io::repr(value as *const Text, false, out),
            b'C' | b'E' => {
                let r = value as *const Record;
                out.extend_from_slice(ty.name.as_bytes());
                let count = record_count(ty, (*r).tag_or_hook);
                if ty.kind == b'E' {
                    out.push(b'.');
                    out.extend_from_slice(ty.variants[(*r).tag_or_hook as usize].name.as_bytes());
                }
                if count > 0 || ty.kind == b'C' {
                    out.push(b'(');
                    for i in 0..count {
                        if i != 0 {
                            out.extend_from_slice(b", ");
                        }
                        if ty.kind == b'C' {
                            out.extend_from_slice(ty.variants[i].name.as_bytes());
                            out.push(b'=');
                        }
                        render(
                            *std::ptr::addr_of!((*r).fields).cast::<u128>().add(i),
                            field_type(ty, (*r).tag_or_hook, i),
                            out,
                        );
                    }
                    out.push(b')');
                }
            }
            _ => {
                let c = &*(value as *const Collection);
                if ty.kind == b'R' {
                    write!(out, "range({}, {}, {})", c.start, c.stop, c.step).unwrap();
                    return;
                }
                if ty.kind == b'S' && c.entries.is_empty() {
                    out.extend_from_slice(b"set()");
                    return;
                }
                out.push(if ty.kind == b'L' { b'[' } else { b'{' });
                for (i, entry) in c.entries.iter().enumerate() {
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
    // SAFETY: generated code supplies three aligned slots and one output slot.
    unsafe {
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
        if !descriptor.is_null() && (*descriptor).kind == b's' {
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
            37 => match strings::try_get(a as *const Text, b as i64) {
                Ok(Some(text)) => wrap(wrap(text as u128, 1), 0),
                Ok(None) => wrap(wrap(0, 0), 0),
                Err(error) => wrap(wrap(0, error as u64), 1),
            },
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
                    release(c.entries[i].key, c.ty().key());
                    c.entries[i].key = value;
                } else {
                    c.insert(b, value);
                }
                plenty_retain(a as *mut Header);
                a
            }
            4 | 6 => {
                let c = &*(a as *const Collection);
                if op == 4 && c.ty().kind == b'D' {
                    let i = c
                        .find(b)
                        .unwrap_or_else(|| crate::fail("dictionary key not found"));
                    retain(c.entries[i].value, c.ty().value());
                    c.entries[i].value
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
                std::mem::take(&mut c.entries[i].key)
            }
            7 => {
                let c = &*(b as *const Collection);
                if c.ty().kind == b'R' {
                    let delta = a as i64 as i128 - c.start as i128;
                    (c.len() > 0
                        && delta % c.step as i128 == 0
                        && delta / c.step as i128 >= 0
                        && delta / (c.step as i128) < c.len() as i128) as u128
                } else if c.ty().kind != b'L' {
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
                let mut out = Vec::new();
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
                crate::io::output(&out);
                0
            }
            10 => {
                let c = collection_new(&*descriptor);
                (*c).start = a as i64;
                (*c).stop = b as i64;
                (*c).step = value as i64;
                if (*c).step == 0 {
                    crate::fail("range step cannot be zero");
                }
                let distance = if (*c).step > 0 {
                    (*c).stop as i128 - (*c).start as i128
                } else {
                    (*c).start as i128 - (*c).stop as i128
                };
                let step = ((*c).step as i128).abs();
                let len = if distance <= 0 {
                    0
                } else {
                    (distance + step - 1) / step
                };
                if len > i64::MAX as i128 {
                    crate::fail("range length exceeds i64");
                }
                (*c).range_len = len as usize;
                c as u128
            }
            11 => {
                let c = &mut *(a as *mut Collection);
                let element = c.ty().value.unwrap();
                let out = collection_new(&*descriptor);
                for entry in &mut c.entries {
                    (*out).insert(entry.value, 0);
                    if element.affine {
                        release(entry.value, element);
                        entry.value = 0;
                    }
                }
                out as u128
            }
            20 | 30 => record_new(&*descriptor, a as u64) as u128,
            21 => {
                let r = a as *mut Record;
                let ty = &*(*r).ty;
                let i = index(b as i64, record_count(ty, (*r).tag_or_hook));
                let field_ty = field_type(ty, (*r).tag_or_hook, i);
                let slot = std::ptr::addr_of_mut!((*r).fields).cast::<u128>().add(i);
                retain(value, field_ty);
                release(*slot, field_ty);
                *slot = value;
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
                let slot = std::ptr::addr_of_mut!((*r).fields).cast::<u128>().add(i);
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

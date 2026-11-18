//! Native builtin owners whose object and row storage use a selected allocator.
//! Selected boxes also hold compiler-certified custom allocator provider state.
use super::*;
use crate::allocator::{Allocator, Block};
use std::alloc::Layout;
use std::mem::ManuallyDrop;

/// Keep the ordinary object header/content offsets unchanged. Only selected
/// objects have this prefix, which owns the allocation containing itself.
#[repr(C, align(16))]
struct Owner {
    block: ManuallyDrop<Block>,
    ty: &'static Type,
}
const _: () = {
    assert!(align_of::<Boxed>() <= align_of::<Owner>());
    assert!(align_of::<Collection>() <= align_of::<Owner>());
};

fn allocate<H, E>(
    allocator: &Allocator,
    ty: &'static Type,
    count: usize,
) -> Result<*mut H, AllocError> {
    let object = memory::object_layout::<H, E>(count)?;
    let (whole, offset) = Layout::new::<Owner>()
        .extend(object)
        .map_err(|_| AllocError::CapacityOverflow)?;
    // Only Boxed/u128 and Collection/u8 call this helper. The fixed prefix is
    // recoverable from either header, including after a queued child finishes.
    assert_eq!(offset, size_of::<Owner>());
    let mut block = allocator.allocate(whole.pad_to_align())?;
    // SAFETY: the validated block contains both aligned regions. Owner keeps
    // that exact block alive; typed consumers expose no safe byte views of it.
    unsafe {
        let base = block.as_mut_ptr();
        base.cast::<Owner>().write(Owner {
            block: ManuallyDrop::new(block),
            ty,
        });
        Ok(base.add(offset).cast())
    }
}
unsafe fn owner(header: *mut Header) -> *mut Owner {
    // SAFETY: only the selected-owner destroy callbacks call this helper.
    unsafe { header.cast::<u8>().sub(size_of::<Owner>()).cast() }
}
unsafe fn free(header: *mut Header) {
    // SAFETY: content has finished destruction. Move the block out before its
    // release clears/frees the allocation that also holds Owner and the header.
    unsafe { drop(ManuallyDrop::take(&mut (*owner(header)).block)) };
}

/// Same owned-input contract as try_box_new: failure leaves value with caller.
unsafe fn try_box_new_in(
    allocator: &Allocator,
    ty: &'static Type,
    value: u128,
) -> Result<*mut Boxed, AllocError> {
    let content = ty.key();
    let boxed = allocate::<Boxed, u128>(allocator, ty, content.field_words())?;
    // SAFETY: layout and type match; the caller transfers one initialized value.
    unsafe {
        boxed.write(Boxed {
            header: Header::new(box_destroy),
            slot: [],
        });
        let field = Boxed::content(boxed, content);
        if !field.flat {
            ranges::store(field.at, value, content);
        } else if value as u64 == 0 {
            field.clear(content);
        } else {
            ranges::copy_payload(value, content, field.at.cast());
        }
    }
    Ok(boxed)
}
/// Transactional compiler callers keep the original value live until commit.
pub(crate) unsafe fn box_new_in(
    allocator: &Allocator,
    ty: &'static Type,
    value: u128,
) -> Result<*mut Header, AllocError> {
    unsafe { try_box_new_in(allocator, ty, value).map(|boxed| boxed.cast()) }
}

/// Rollback must not destroy the relocated provisional copy of the provider.
pub(crate) unsafe fn provider_box_rollback(header: *mut Header, ty: &Type) {
    unsafe { box_take(header.cast(), ty) };
}

unsafe extern "C" fn box_destroy(header: *mut Header) {
    unsafe {
        let ty = (*owner(header)).ty;
        // Provider release is observable even when element types have no custom
        // destructor. Never free this owner ahead of its queued children.
        if release_box_content(header, ty, true) {
            free(header);
        }
    }
}

pub(crate) fn list_new_in(
    allocator: &Allocator,
    ty: &'static Type,
    capacity: usize,
) -> Result<*mut Header, AllocError> {
    try_list_new_in(allocator, ty, capacity).map(|list| list.cast())
}

fn try_list_new_in(
    allocator: &Allocator,
    ty: &'static Type,
    capacity: usize,
) -> Result<*mut Collection, AllocError> {
    assert!(
        ty.kind == b'L' && ty.value.is_none(),
        "selected dense list descriptor"
    );
    // Validate the initial request before allocating bookkeeping or invoking a
    // provider, matching the existing collection capacity/row-layout checks.
    memory::checked_capacity::<Entry>(capacity)?;
    let key = ty.key();
    let stride =
        crate::list_layout::row_bytes(key.kind, key.inline_bytes as usize, key.has_inline_range());
    let bytes = capacity
        .checked_mul(stride)
        .ok_or(AllocError::CapacityOverflow)?;
    Layout::array::<u128>(bytes.div_ceil(16)).map_err(|_| AllocError::CapacityOverflow)?;
    let entries = Entries::new_in(ty.key(), allocator.clone())?;
    try_collection_with_entries_in(allocator, ty, capacity, entries)
}

pub(crate) fn dict_set_new_in(
    allocator: &Allocator,
    ty: &'static Type,
    capacity: usize,
) -> Result<*mut Header, AllocError> {
    try_dict_set_new_in(allocator, ty, capacity).map(|set| set.cast())
}

fn try_dict_set_new_in(
    allocator: &Allocator,
    ty: &'static Type,
    capacity: usize,
) -> Result<*mut Collection, AllocError> {
    if ty.kind == b'S' && crate::entries::enum_bits::eligible(ty.key()) {
        // The mask lives in the selected object itself. There are no row or
        // bucket blocks to reserve, nor a separate allocator handle for growth.
        try_collection_with_entries_in(allocator, ty, capacity, Entries::enum_set(ty.key()))
    } else {
        try_hash_new_in(allocator, ty, capacity)
    }
}

/// Both borrows must be ordinary sets of the same legal, copyable key type.
/// Key views need a separate result descriptor. Affine keys are excluded:
/// `try_push_members` calls `try_copy_into` for those, and recursive box and
/// collection copies still allocate implicitly. Retaining a shared key does
/// not relocate its existing backing storage to the result allocator.
unsafe fn try_set_algebra_in(
    allocator: &Allocator,
    left: &Collection,
    right: &Collection,
    op: i64,
) -> Result<u128, AllocError> {
    let ty = unsafe { &*left.ty };
    assert!(ty.kind == b'S' && right.ty().kind == b'S');
    assert!(
        !ty.key().affine,
        "selected algebra requires non-affine keys"
    );
    assert!(matches!(op, SET_UNION..=SET_SYMMETRIC_DIFFERENCE));
    unsafe {
        try_set_algebra_with(left, right, op, ty, |ty, count| {
            try_dict_set_new_in(allocator, ty, count)
        })
    }
}

pub(super) fn try_hash_new_in(
    allocator: &Allocator,
    ty: &'static Type,
    capacity: usize,
) -> Result<*mut Collection, AllocError> {
    assert!(
        ty.kind == b'D' || (ty.kind == b'S' && !crate::entries::enum_bits::eligible(ty.key())),
        "selected hash descriptor"
    );
    memory::checked_capacity::<Entry>(capacity)?;
    Buckets::check_selected_capacity(capacity)?;
    let entries = Entries::ordered_in(ty.key(), ty.value, capacity, allocator)?;
    try_collection_with_entries_in(allocator, ty, capacity, entries)
}

fn try_collection_with_entries_in(
    allocator: &Allocator,
    ty: &'static Type,
    capacity: usize,
    entries: Entries,
) -> Result<*mut Collection, AllocError> {
    let mut collection = Collection {
        header: Header::new(collection_destroy_in),
        ty,
        entries,
        table: Buckets::new(),
        drop_entry: None,
    };
    // As on the legacy path, partial construction holds no initialized elements.
    // A failed object allocation drops the reserved buffer through its provider.
    unsafe { collection.try_reserve(capacity)? };
    let pointer = allocate::<Collection, u8>(allocator, ty, 0)?;
    unsafe { pointer.write(collection) };
    Ok(pointer)
}
unsafe extern "C" fn collection_destroy_in(header: *mut Header) {
    unsafe {
        if release_collection_content(header) {
            free(header);
        }
    }
}

#[cfg(test)]
mod tests;

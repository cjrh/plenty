//! Inline environments have no allocation header. Their type descriptor is at
//! byte zero, followed by padding and typed capture slots starting at byte 16.
use crate::aggregates::{self, Type};
use crate::ranges;

unsafe fn fields(value: *mut u128, mut visit: impl FnMut(*mut u128, &Type)) {
    if value.is_null() {
        return;
    }
    // SAFETY: code generation writes a static descriptor before publishing an
    // environment. Each slot reserves its descriptor's complete inline payload.
    unsafe {
        let ty = &**value.cast::<*const Type>();
        let mut slot = value.add(1);
        for field in ty.variants[0].fields {
            visit(slot, field);
            slot = slot.add(field.slot_words());
        }
    }
}

pub(crate) unsafe fn retain(value: *mut u128) {
    unsafe {
        fields(value, |slot, ty| aggregates::retain(slot.read(), ty));
    }
}

pub(crate) unsafe fn release(value: *mut u128) {
    // Reverse destruction needs no temporary allocation: recover offsets from
    // the immutable descriptor, then walk the slots backwards.
    if value.is_null() {
        return;
    }
    unsafe {
        let ty = &**value.cast::<*const Type>();
        let fields = ty.variants[0].fields;
        let mut slot = value.add(1 + fields.iter().map(|t| t.slot_words()).sum::<usize>());
        for field in fields.iter().rev() {
            slot = slot.sub(field.slot_words());
            aggregates::release(slot.read(), field);
        }
    }
}

pub(crate) unsafe fn relocate(value: *mut u128) {
    unsafe {
        fields(value, |slot, ty| ranges::relocate(slot, ty));
    }
}

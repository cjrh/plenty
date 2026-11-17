//! Private compiler ABI. An owning handle is one retained ProviderHeader pointer;
//! null is the explicit system root. No public raw-pointer constructor exists.
use super::*;
use crate::aggregates::{Type, allocator_owners};
use crate::collection_opcodes::*;

#[inline(always)]
pub(crate) unsafe fn operation(op: i64, a: u128, b: u128, ty: *const Type) -> u128 {
    // Check before touching ownership or allocating. Separately compiled hosts
    // must also keep retained owners out of actor/arena contexts.
    if !crate::actor_heap::allocator_registration_allowed() {
        crate::fail("retained allocator use is unsupported in actor or arena contexts");
    }
    let into_handle = |allocator: Allocator| {
        let pointer = allocator.0.map_or(std::ptr::null_mut(), NonNull::as_ptr);
        std::mem::forget(allocator);
        pointer as u128
    };
    // SAFETY: generated std code passes null or a live retained header. This
    // temporary only borrows that reference except for the explicit release.
    let handle = ManuallyDrop::new(Allocator(NonNull::new(a as *mut ProviderHeader)));
    match op {
        ALLOCATOR_SYSTEM => into_handle(Allocator::system()),
        ALLOCATOR_SHARE => into_handle(Allocator::clone(&handle)),
        ALLOCATOR_RELEASE => {
            drop(ManuallyDrop::into_inner(handle));
            0
        }
        ALLOCATOR_LIST => {
            let result = if (b as i64) < 0 {
                Err(AllocError::CapacityOverflow)
            } else {
                // SAFETY: compiler emits a static dense-list descriptor.
                allocator_owners::list_new_in(&handle, unsafe { &*ty }, b as usize)
            };
            match result {
                Ok(list) => list as u128,
                Err(error) => (1 + 2 * error as u128) << 64,
            }
        }
        _ => unreachable!("retained allocator opcode"),
    }
}

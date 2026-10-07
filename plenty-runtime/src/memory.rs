//! Intrusive, reentrant destruction without recursion through owned object graphs.
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::cell::Cell;
use std::ptr;

/// Tags match the compiler's allocation-free AllocError builtin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AllocError {
    OutOfMemory = 0,
    CapacityOverflow = 1,
}

/// Validate separately so stable Vec APIs need not expose TryReserveErrorKind.
pub(crate) fn checked_capacity<T>(count: usize) -> Result<usize, AllocError> {
    Layout::array::<T>(count).map_err(|_| AllocError::CapacityOverflow)?;
    Ok(count)
}

pub(crate) fn try_reserve<T>(buffer: &mut Vec<T>, additional: usize) -> Result<(), AllocError> {
    let count = buffer
        .len()
        .checked_add(additional)
        .ok_or(AllocError::CapacityOverflow)?;
    checked_capacity::<T>(count)?;
    buffer
        .try_reserve_exact(additional)
        .map_err(|_| AllocError::OutOfMemory)
}

#[repr(C)]
pub(crate) struct Header {
    pub refs: u64,
    // None is valid for compiler-emitted immortal string literals.
    pub destroy: Option<unsafe extern "C" fn(*mut Header)>,
}
impl Header {
    pub fn new(destroy: unsafe extern "C" fn(*mut Header)) -> Self {
        Self {
            refs: 1,
            destroy: Some(destroy),
        }
    }
}
const _: () = assert!(size_of::<Header>() == 16);

#[derive(Clone, Copy, Default)]
struct Queue {
    pending: *mut Header,
    dropping: bool,
}
thread_local! { static QUEUE: Cell<Queue> = const { Cell::new(Queue { pending: ptr::null_mut(), dropping: false }) }; }

pub(crate) fn dropping() -> bool {
    QUEUE.with(|q| q.get().dropping)
}

/// A user hook's explicit drops finish synchronously before its next statement.
/// Never hold a RefCell borrow or mutable Rust reference across native callbacks.
pub(crate) fn with_nested_drops(f: impl FnOnce()) {
    struct Restore(Queue);
    impl Drop for Restore {
        fn drop(&mut self) {
            QUEUE.with(|q| q.set(self.0));
        }
    }
    let _restore = Restore(QUEUE.with(|q| q.replace(Queue::default())));
    f();
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_retain(object: *mut Header) {
    // SAFETY: callers supply a live object or the null moved-slot sentinel.
    unsafe {
        if object.is_null() || (*object).refs == u64::MAX {
            return;
        }
        if (*object).refs == u64::MAX - 1 {
            crate::fail("reference count overflow");
        }
        (*object).refs += 1;
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_release(object: *mut Header) {
    // SAFETY: each caller consumes exactly one live owned reference. The dead
    // object's count stores the queue link until its destruction callback runs.
    unsafe {
        if object.is_null() || (*object).refs == u64::MAX {
            return;
        }
        debug_assert!((*object).refs > 0);
        (*object).refs -= 1;
        if (*object).refs != 0 {
            return;
        }
        let should_drain = QUEUE.with(|q| {
            let mut state = q.get();
            (*object).refs = state.pending as u64;
            state.pending = object;
            let drain = !state.dropping;
            state.dropping = true;
            q.set(state);
            drain
        });
        if !should_drain {
            return;
        }
        loop {
            let next = QUEUE.with(|q| {
                let mut state = q.get();
                let next = state.pending;
                if next.is_null() {
                    state.dropping = false;
                } else {
                    state.pending = (*next).refs as *mut Header;
                }
                q.set(state);
                next
            });
            if next.is_null() {
                break;
            }
            ((*next).destroy.expect("owned object destructor"))(next);
        }
    }
}

fn try_flex_layout<H, E>(count: usize) -> Result<Layout, AllocError> {
    Layout::array::<E>(count)
        .ok()
        .and_then(|tail| Layout::new::<H>().extend(tail).ok())
        .map(|(layout, _)| layout.pad_to_align())
        .ok_or(AllocError::CapacityOverflow)
}
/// Internal allocator identity. Callbacks obey Rust Layout allocation contracts;
/// storage must be zeroed and the allocator must outlive all of its allocations.
pub(crate) struct Allocator {
    allocate_zeroed: unsafe fn(Layout) -> *mut u8,
    deallocate: unsafe fn(*mut u8, Layout),
}
static GLOBAL: Allocator = Allocator {
    allocate_zeroed: alloc_zeroed,
    deallocate: dealloc,
};

#[repr(C)]
struct Allocation {
    allocator: &'static Allocator,
}

fn allocation_layout<H, E>(count: usize) -> Result<(Layout, usize), AllocError> {
    let object = try_flex_layout::<H, E>(count)?;
    assert!(
        object.size() != 0,
        "runtime allocations require a nonzero header"
    );
    Layout::new::<Allocation>()
        .extend(object)
        .map(|(layout, offset)| (layout.pad_to_align(), offset))
        .map_err(|_| AllocError::CapacityOverflow)
}

pub(crate) fn try_allocate<H, E>(count: usize) -> Result<*mut H, AllocError> {
    try_allocate_in::<H, E>(count, &GLOBAL)
}

pub(crate) fn try_allocate_in<H, E>(
    count: usize,
    allocator: &'static Allocator,
) -> Result<*mut H, AllocError> {
    let (layout, offset) = allocation_layout::<H, E>(count)?;
    // SAFETY: the checked layout has a nonzero size. Null means no allocation
    // was obtained, so returning an error leaves nothing to deallocate.
    let pointer = unsafe { (allocator.allocate_zeroed)(layout) };
    if pointer.is_null() {
        Err(AllocError::OutOfMemory)
    } else {
        unsafe {
            pointer.cast::<Allocation>().write(Allocation { allocator });
            Ok(pointer.add(offset).cast())
        }
    }
}
pub(crate) unsafe fn free<H, E>(pointer: *mut H, count: usize) {
    // SAFETY: pointer/count match try_allocate[_in]; the preceding private
    // prefix remains live until the allocator that created it reclaims it.
    unsafe {
        let (layout, offset) = allocation_layout::<H, E>(count).expect("live allocation layout");
        let base = pointer.cast::<u8>().sub(offset);
        let allocator = (*base.cast::<Allocation>()).allocator;
        (allocator.deallocate)(base, layout);
    }
}

#[cfg(test)]
mod allocator_tests {
    use super::*;
    thread_local! { static FREED: Cell<usize> = const { Cell::new(0) }; }
    unsafe fn custom_free(pointer: *mut u8, layout: Layout) {
        FREED.with(|n| n.set(n.get() + 1));
        unsafe {
            dealloc(pointer, layout);
        }
    }
    static CUSTOM: Allocator = Allocator {
        allocate_zeroed: alloc_zeroed,
        deallocate: custom_free,
    };
    #[repr(C, align(64))]
    struct Aligned([u8; 64]);
    #[test]
    fn provenance_and_alignment_survive_moving_the_owner() {
        FREED.with(|n| n.set(0));
        let first = try_allocate_in::<Aligned, u128>(3, &CUSTOM).unwrap();
        let second = try_allocate::<Aligned, u128>(3).unwrap();
        assert_eq!(first as usize % 64, 0);
        unsafe {
            assert!((*first).0.iter().all(|b| *b == 0));
            free::<Aligned, u128>(second, 3);
            assert_eq!(FREED.with(Cell::get), 0);
            free::<Aligned, u128>(first, 3);
        }
        assert_eq!(FREED.with(Cell::get), 1);
    }
}

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
pub(crate) fn flex_layout<H, E>(count: usize) -> Layout {
    try_flex_layout::<H, E>(count).unwrap_or_else(|_| crate::fail("allocation capacity overflow"))
}
pub(crate) fn try_allocate<H, E>(count: usize) -> Result<*mut H, AllocError> {
    let layout = try_flex_layout::<H, E>(count)?;
    assert!(
        layout.size() != 0,
        "runtime allocations require a nonzero header"
    );
    // SAFETY: the checked layout has a nonzero size. Null means no allocation
    // was obtained, so returning an error leaves nothing to deallocate.
    let pointer = unsafe { alloc_zeroed(layout) };
    if pointer.is_null() {
        Err(AllocError::OutOfMemory)
    } else {
        Ok(pointer.cast())
    }
}
pub(crate) unsafe fn free<H, E>(pointer: *mut H, count: usize) {
    // SAFETY: pointer/count must match allocate; all owned payloads are released.
    unsafe {
        dealloc(pointer.cast(), flex_layout::<H, E>(count));
    }
}

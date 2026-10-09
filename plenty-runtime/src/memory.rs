//! Intrusive, reentrant destruction without recursion through owned object graphs.
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::cell::Cell;
use std::ptr;
use std::sync::atomic::{fence, AtomicU64, Ordering};

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
    pub refs: AtomicU64,
    // None is valid for compiler-emitted immortal string literals.
    pub destroy: Option<unsafe extern "C" fn(*mut Header)>,
}
impl Header {
    pub fn new(destroy: unsafe extern "C" fn(*mut Header)) -> Self {
        Self {
            refs: AtomicU64::new(1),
            destroy: Some(destroy),
        }
    }

    /// Consume one count and synchronize with every earlier release before the
    /// final owner destroys the payload. This does not synchronize payload use.
    pub(crate) fn release_last(&self) -> bool {
        if self.refs.load(Ordering::Relaxed) == u64::MAX {
            return false;
        }
        let previous = self.refs.fetch_sub(1, Ordering::Release);
        debug_assert!(previous > 0);
        if previous != 1 {
            return false;
        }
        fence(Ordering::Acquire);
        true
    }
}
const _: () = {
    assert!(size_of::<Header>() == 16);
    assert!(align_of::<Header>() == 8);
    assert!(std::mem::offset_of!(Header, destroy) == 8);
};

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

/// The null moved-slot sentinel and inline strings (low bit set) own nothing.
fn inert(object: *mut Header) -> bool {
    object.is_null() || object as usize & 1 != 0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_retain(object: *mut Header) {
    // SAFETY: callers supply a live object or the null moved-slot sentinel.
    unsafe {
        if inert(object) || (*object).refs.load(Ordering::Relaxed) == u64::MAX {
            return;
        }
        // An existing live owner permits relaxed retain. Reject overflow
        // before incrementing so concurrent retains cannot reach the sentinel.
        if (*object)
            .refs
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < u64::MAX - 1).then(|| count + 1)
            })
            .is_err()
        {
            crate::fail("reference count overflow");
        }
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_release(object: *mut Header) {
    // SAFETY: each caller consumes exactly one live owned reference. The dead
    // object's count stores the queue link until its destruction callback runs.
    unsafe {
        if inert(object) || !(*object).release_last() {
            return;
        }
        let should_drain = QUEUE.with(|q| {
            let mut state = q.get();
            // No other owner remains after the acquire handoff. Queue links
            // are thread-local and can now reuse the dead count word.
            (*object)
                .refs
                .store(state.pending as u64, Ordering::Relaxed);
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
                    state.pending = (*next).refs.load(Ordering::Relaxed) as *mut Header;
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

#[cfg(test)]
mod sharing_tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, Arc, Barrier};

    #[repr(C)]
    struct Shared {
        header: Header,
        destroyed: Arc<AtomicUsize>,
        writes: [AtomicUsize; 4],
    }

    unsafe extern "C" fn destroy(header: *mut Header) {
        // SAFETY: the sole final release owns this Box and has acquired every
        // worker's preceding release. The count itself is now a queue link.
        let object = unsafe { Box::from_raw(header.cast::<Shared>()) };
        for slot in &object.writes {
            assert_eq!(slot.load(Ordering::Relaxed), 1);
        }
        assert_eq!(object.destroyed.fetch_add(1, Ordering::Relaxed), 0);
    }

    #[test]
    fn concurrent_retain_release_has_one_synchronized_final_destructor() {
        for _ in 0..32 {
            let destroyed = Arc::new(AtomicUsize::new(0));
            let object = Box::into_raw(Box::new(Shared {
                header: Header::new(destroy),
                destroyed: destroyed.clone(),
                writes: std::array::from_fn(|_| AtomicUsize::new(0)),
            }));
            let barrier = Barrier::new(4);
            // Allocate one live ownership count per worker before publishing
            // the exposed address. All shared payload writes are atomic.
            for _ in 1..4 {
                unsafe { plenty_retain(object.cast()) };
            }
            let address = object.expose_provenance();
            std::thread::scope(|scope| {
                for worker in 0..4 {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        let object = ptr::with_exposed_provenance_mut::<Shared>(address);
                        barrier.wait();
                        for _ in 0..1000 {
                            unsafe {
                                plenty_retain(object.cast());
                                plenty_release(object.cast());
                            }
                        }
                        unsafe {
                            (*object).writes[worker].store(1, Ordering::Relaxed);
                            plenty_release(object.cast());
                        }
                    });
                }
            });
            assert_eq!(destroyed.load(Ordering::Relaxed), 1);
        }
    }
}

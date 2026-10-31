//! Darwin's ulock calls are private APIs; Rust's Darwin thread parker records
//! the same restriction in library/std/src/sys/sync/thread_parking/darwin.rs.
//! Use public pthread primitives with static storage instead. This preserves
//! the allocation-free mutex contract, unlike a lazily allocated std Condvar.
use super::*;
use std::cell::UnsafeCell;

struct Parking {
    gate: UnsafeCell<libc::pthread_mutex_t>,
    changed: UnsafeCell<libc::pthread_cond_t>,
}

// SAFETY: pthread primitives synchronize their internal mutable state. The
// objects stay at a fixed address and are never destroyed during execution.
unsafe impl Sync for Parking {}

static PARKING: Parking = Parking {
    gate: UnsafeCell::new(libc::PTHREAD_MUTEX_INITIALIZER),
    changed: UnsafeCell::new(libc::PTHREAD_COND_INITIALIZER),
};

pub(super) fn wait(state: &AtomicU32) {
    // SAFETY: the static primitives are initialized and never moved. The gate
    // pairs predicate inspection with wake registration so an unlock cannot
    // slip between checking the state and sleeping. Every wait uses this gate.
    unsafe {
        if libc::pthread_mutex_lock(PARKING.gate.get()) != 0 {
            crate::fail("mutex parking lock failed");
        }
        if state.load(Ordering::Relaxed) == 2
            && libc::pthread_cond_wait(PARKING.changed.get(), PARKING.gate.get()) != 0
        {
            crate::fail("mutex parking wait failed");
        }
        if libc::pthread_mutex_unlock(PARKING.gate.get()) != 0 {
            crate::fail("mutex parking unlock failed");
        }
    }
}

pub(super) fn wake(_: &AtomicU32) {
    // SAFETY: as above. Different mutex words share the condition variable, so
    // broadcast is required: waking one unrelated waiter could strand the
    // intended waiter. The global gate serializes only parking, not workloads.
    unsafe {
        if libc::pthread_mutex_lock(PARKING.gate.get()) != 0 {
            crate::fail("mutex parking lock failed");
        }
        if libc::pthread_cond_broadcast(PARKING.changed.get()) != 0 {
            crate::fail("mutex parking wake failed");
        }
        if libc::pthread_mutex_unlock(PARKING.gate.get()) != 0 {
            crate::fail("mutex parking unlock failed");
        }
    }
}

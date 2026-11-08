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

/// Sleep while `word` holds `expected`, at most `timeout`. A timed sleep
/// uses the realtime clock that `pthread_cond_timedwait` measures; a clock
/// step can lengthen or shorten it, and `lock_when` callers recheck the
/// monotonic deadline after every return.
pub(super) fn wait(word: &AtomicU32, expected: u32, timeout: Option<Duration>) {
    // SAFETY: the static primitives are initialized and never moved. The gate
    // pairs predicate inspection with wake registration so a change cannot
    // slip between checking the word and sleeping. Every wait uses this gate.
    unsafe {
        if libc::pthread_mutex_lock(PARKING.gate.get()) != 0 {
            crate::fail("mutex parking lock failed");
        }
        if word.load(Ordering::Relaxed) == expected {
            let status = match timeout {
                None => libc::pthread_cond_wait(PARKING.changed.get(), PARKING.gate.get()),
                Some(timeout) => {
                    let until = realtime_after(timeout);
                    libc::pthread_cond_timedwait(PARKING.changed.get(), PARKING.gate.get(), &until)
                }
            };
            if status != 0 && status != libc::ETIMEDOUT {
                crate::fail("mutex parking wait failed");
            }
        }
        if libc::pthread_mutex_unlock(PARKING.gate.get()) != 0 {
            crate::fail("mutex parking unlock failed");
        }
    }
}

fn realtime_after(timeout: Duration) -> libc::timespec {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid output location.
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) } != 0 {
        crate::fail("mutex parking clock failed");
    }
    let nanoseconds = now.tv_nsec as u64 + u64::from(timeout.subsec_nanos());
    libc::timespec {
        tv_sec: now.tv_sec
            + timeout.as_secs() as libc::time_t
            + (nanoseconds / 1_000_000_000) as libc::time_t,
        tv_nsec: (nanoseconds % 1_000_000_000) as _,
    }
}

/// Every parked thread wakes and rechecks its word, so `all` changes nothing.
pub(super) fn wake(_: &AtomicU32, _all: bool) {
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

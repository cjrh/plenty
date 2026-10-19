//! Contended paths of `std.sync.Mutex`. Generated code owns the fast paths:
//! lock is an inline compare-exchange of the state word from 0 (unlocked) to 1
//! (locked), and unlock an inline exchange to 0. A failed lock calls
//! [`plenty_mutex_lock_slow`]; an unlock that replaced 2 (locked with possible
//! waiters) calls [`plenty_mutex_wake`]. This is mutex 2 of Drepper's
//! "Futexes Are Tricky". Nothing allocates, and a trap never unwinds through a
//! held lock, so there is no poisoned state.
use std::sync::atomic::{AtomicU32, Ordering};

// The x86_64 Linux GNU ABI, the compiler's only supported native target.
const SYS_FUTEX: i64 = 202;
const FUTEX_WAIT_PRIVATE: i64 = 128;
const FUTEX_WAKE_PRIVATE: i64 = 129;
const EINTR: i32 = 4;
const EAGAIN: i32 = 11;

unsafe extern "C" {
    fn syscall(number: i64, ...) -> i64;
}

/// Acquire a lock whose inline compare-exchange failed. Each attempt marks the
/// state as having waiters, so the holder's unlock always wakes one.
///
/// # Safety
///
/// `state` is the live, aligned state word of a `Mutex` that no thread moves
/// or drops while any thread can lock it.
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_mutex_lock_slow(state: *const AtomicU32) {
    // SAFETY: the caller's loans keep the mutex in place while it is locked.
    let state = unsafe { &*state };
    while state.swap(2, Ordering::Acquire) != 0 {
        // SAFETY: the futex word is valid for this call; a private wait
        // sleeps only while the word still holds 2.
        let status = unsafe {
            syscall(
                SYS_FUTEX,
                state.as_ptr(),
                FUTEX_WAIT_PRIVATE,
                2_i64,
                std::ptr::null::<u8>(),
            )
        };
        // EAGAIN means the word changed before sleeping, and EINTR a signal;
        // both retry the exchange, as does a wakeup.
        if status != 0 {
            let error = std::io::Error::last_os_error().raw_os_error();
            if !matches!(error, Some(EINTR | EAGAIN)) {
                crate::fail("mutex futex wait failed");
            }
        }
    }
}

/// Wake one waiter after an unlock that replaced state 2.
///
/// # Safety
///
/// As for [`plenty_mutex_lock_slow`].
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_mutex_wake(state: *const AtomicU32) {
    // SAFETY: as for the wait; waking reads no memory beyond the word's address.
    let status = unsafe { syscall(SYS_FUTEX, (*state).as_ptr(), FUTEX_WAKE_PRIVATE, 1_i64) };
    if status < 0 {
        crate::fail("mutex futex wake failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::UnsafeCell;

    struct Guarded {
        state: AtomicU32,
        value: UnsafeCell<(u64, u64)>,
    }

    // SAFETY: `value` is accessed only while `state` is locked.
    unsafe impl Sync for Guarded {}

    /// The same protocol generated code inlines around the runtime calls.
    fn locked(guarded: &Guarded, update: impl FnOnce(&mut (u64, u64))) {
        if guarded
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // SAFETY: `guarded` outlives every thread in the test.
            unsafe { plenty_mutex_lock_slow(&guarded.state) };
        }
        // SAFETY: the lock is held.
        update(unsafe { &mut *guarded.value.get() });
        if guarded.state.swap(0, Ordering::Release) == 2 {
            // SAFETY: as above.
            unsafe { plenty_mutex_wake(&guarded.state) };
        }
    }

    #[test]
    fn contended_updates_keep_both_fields_together() {
        const WORKERS: u64 = 8;
        const ROUNDS: u64 = 20_000;
        let guarded = Guarded {
            state: AtomicU32::new(0),
            value: UnsafeCell::new((0, 0)),
        };
        std::thread::scope(|scope| {
            for _ in 0..WORKERS {
                let guarded = &guarded;
                scope.spawn(move || {
                    for _ in 0..ROUNDS {
                        locked(guarded, |(count, double)| {
                            assert_eq!(*double, 2 * *count);
                            *count += 1;
                            std::hint::spin_loop();
                            *double += 2;
                        });
                    }
                });
            }
        });
        let (count, double) = guarded.value.into_inner();
        assert_eq!((count, double), (WORKERS * ROUNDS, 2 * WORKERS * ROUNDS));
        assert_eq!(guarded.state.into_inner(), 0);
    }
}

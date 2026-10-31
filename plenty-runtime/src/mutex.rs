//! Contended paths of `std.sync.Mutex`. Generated code owns the fast paths:
//! lock is an inline compare-exchange of the state word from 0 (unlocked) to 1
//! (locked), and unlock an inline exchange to 0. A failed lock calls
//! [`plenty_mutex_lock_slow`]; an unlock that replaced 2 (locked with possible
//! waiters) calls [`plenty_mutex_wake`]. This is mutex 2 of Drepper's
//! "Futexes Are Tricky". Nothing allocates, and a trap never unwinds through a
//! held lock, so there is no poisoned state.
//!
//! Linux waits use futexes; Apple and Windows use platform backends. The
//! state word and generated fast paths have the same ABI everywhere.
//!
//! Native ABI bindings use the [libc crate](https://github.com/rust-lang/libc),
//! licensed MIT OR Apache-2.0.
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(target_os = "linux")]
mod native {
    use super::*;

    pub(super) fn wait(state: &AtomicU32) {
        // SAFETY: the caller keeps the aligned state word live. A private
        // futex wait sleeps only while the word still holds 2.
        let status = unsafe {
            libc::syscall(
                libc::SYS_futex,
                state.as_ptr(),
                libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
                2_u32,
                std::ptr::null::<libc::timespec>(),
            )
        };
        if status != 0
            && !matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR | libc::EAGAIN)
            )
        {
            crate::fail("mutex futex wait failed");
        }
    }

    pub(super) fn wake(state: &AtomicU32) {
        // SAFETY: waking reads no memory beyond the live word's address.
        let status = unsafe {
            libc::syscall(
                libc::SYS_futex,
                state.as_ptr(),
                libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
                1_u32,
            )
        };
        if status < 0 {
            crate::fail("mutex futex wake failed");
        }
    }
}

#[cfg(target_os = "windows")]
#[path = "mutex/windows.rs"]
mod native;

#[cfg(target_vendor = "apple")]
#[path = "mutex/apple.rs"]
mod native;

#[cfg(not(any(target_os = "linux", target_os = "windows", target_vendor = "apple")))]
mod native {
    use super::*;

    pub(super) fn wait(_: &AtomicU32) {
        crate::fail("mutex waiting is unsupported on this platform");
    }

    pub(super) fn wake(_: &AtomicU32) {
        crate::fail("mutex waking is unsupported on this platform");
    }
}

/// Acquire a lock whose inline compare-exchange failed. Each attempt stores 2,
/// so the holder's unlock wakes a waiter if there is one. Acquiring through
/// that exchange also leaves 2 when no thread waits, so the next unlock may
/// make one unnecessary `FUTEX_WAKE`; the protocol accepts that cost, and it
/// is correct because a wake with no sleeper does nothing.
///
/// # Safety
///
/// `state` is the live, aligned state word of a `Mutex` that no thread moves
/// or drops while any thread can lock it.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_mutex_lock_slow(state: *const AtomicU32) {
    // SAFETY: the caller's loans keep the mutex in place while it is locked.
    let state = unsafe { &*state };
    while state.swap(2, Ordering::Acquire) != 0 {
        native::wait(state);
    }
}

/// Wake waiters after an unlock that replaced state 2.
///
/// # Safety
///
/// As for [`plenty_mutex_lock_slow`].
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_mutex_wake(state: *const AtomicU32) {
    // SAFETY: the caller keeps the state word live and aligned.
    native::wake(unsafe { &*state });
}

// Exercise the public-pthread implementation on Unix CI even when the host
// uses futexes in production. No platform primitive is replaced in these tests.
#[cfg(all(test, unix, not(target_vendor = "apple")))]
#[path = "mutex/apple.rs"]
mod pthread_test_backend;

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

    #[cfg(unix)]
    #[test]
    fn pthread_parking_handles_multiple_mutex_words() {
        #[cfg(target_vendor = "apple")]
        use super::native as backend;
        #[cfg(not(target_vendor = "apple"))]
        use super::pthread_test_backend as backend;

        let guarded: [_; 4] = std::array::from_fn(|_| Guarded {
            state: AtomicU32::new(0),
            value: UnsafeCell::new((0, 0)),
        });
        std::thread::scope(|scope| {
            for item in &guarded {
                for _ in 0..4 {
                    scope.spawn(move || {
                        for _ in 0..2_000 {
                            if item
                                .state
                                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                                .is_err()
                            {
                                while item.state.swap(2, Ordering::Acquire) != 0 {
                                    backend::wait(&item.state);
                                }
                            }
                            // SAFETY: this worker holds this item's mutex.
                            let value = unsafe { &mut *item.value.get() };
                            assert_eq!(value.0 * 2, value.1);
                            value.0 += 1;
                            std::hint::spin_loop();
                            value.1 += 2;
                            if item.state.swap(0, Ordering::Release) == 2 {
                                backend::wake(&item.state);
                            }
                        }
                    });
                }
            }
        });
        for item in guarded {
            assert_eq!(item.value.into_inner(), (8_000, 16_000));
            assert_eq!(item.state.into_inner(), 0);
        }
    }
}

//! Contended and waiting paths of `std.sync.Mutex`. Generated code owns the
//! fast paths: lock is an inline compare-exchange of the state word from 0
//! (unlocked) to 1 (locked), and unlock an inline exchange to 0. A failed lock
//! calls [`plenty_mutex_lock_slow`]; an unlock that replaced more than 1
//! calls [`plenty_mutex_release`]. Bits 0-1 of the state are mutex 2 of
//! Drepper's "Futexes Are Tricky": 0, 1, or 2 (locked with possible lock
//! waiters). Bit 2, [`WAITING`], records `lock_when` waiters; the unlock that
//! clears it advances the mutex's sequence word and wakes every thread
//! sleeping on that word, so any change made under the lock notifies them.
//! Nothing allocates, and a trap never unwinds through a held lock, so there
//! is no poisoned state.
//!
//! Linux waits use futexes; Apple and Windows use platform backends. The
//! state word and generated fast paths have the same ABI everywhere.
//!
//! Native ABI bindings use the [libc crate](https://github.com/rust-lang/libc),
//! licensed MIT OR Apache-2.0.
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// The lock bits of the state word.
const HELD: u32 = 3;
/// Locked with possible lock waiters.
const CONTENDED: u32 = 2;
/// Threads in `lock_when` may sleep on the sequence word.
const WAITING: u32 = 4;
/// The longest sleep before a `lock_when` returns to check cancellation, as
/// std.sync's `wait_slice` does for the `_before` waits.
const SLICE: Duration = Duration::from_millis(10);

#[cfg(target_os = "linux")]
mod native {
    use super::*;

    /// Sleep while `word` holds `expected`, at most `timeout`.
    pub(super) fn wait(word: &AtomicU32, expected: u32, timeout: Option<Duration>) {
        let span = timeout.map(|timeout| libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: timeout.subsec_nanos() as _,
        });
        let span = span
            .as_ref()
            .map_or(std::ptr::null(), |span| span as *const libc::timespec);
        // SAFETY: the caller keeps the aligned word live, and `span` outlives
        // the call. A private futex wait sleeps only while the word still
        // holds `expected`; its timeout is relative.
        let status = unsafe {
            libc::syscall(
                libc::SYS_futex,
                word.as_ptr(),
                libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
                expected,
                span,
            )
        };
        if status != 0
            && !matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EINTR | libc::EAGAIN | libc::ETIMEDOUT)
            )
        {
            crate::fail("mutex futex wait failed");
        }
    }

    /// Wake one thread sleeping on `word`, or all of them.
    pub(super) fn wake(word: &AtomicU32, all: bool) {
        let count = if all { i32::MAX as u32 } else { 1 };
        // SAFETY: waking reads no memory beyond the live word's address.
        let status = unsafe {
            libc::syscall(
                libc::SYS_futex,
                word.as_ptr(),
                libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
                count,
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

    pub(super) fn wait(_: &AtomicU32, _: u32, _: Option<Duration>) {
        crate::fail("mutex waiting is unsupported on this platform");
    }

    pub(super) fn wake(_: &AtomicU32, _: bool) {
        crate::fail("mutex waking is unsupported on this platform");
    }
}

/// Acquire, sleeping while another thread holds the lock. Each acquisition
/// and each sleep leaves the lock bits at 2, so the holder's unlock wakes a
/// waiter if there is one. Acquiring that way also leaves 2 when no thread
/// waits, so the next unlock may make one unnecessary wake; the protocol
/// accepts that cost, and it is correct because a wake with no sleeper does
/// nothing. [`WAITING`] is kept as found.
fn lock(state: &AtomicU32) {
    let mut current = state.load(Ordering::Relaxed);
    loop {
        let marked = (current & WAITING) | CONTENDED;
        if current & HELD == 0 {
            match state.compare_exchange_weak(current, marked, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => return,
                Err(found) => current = found,
            }
            continue;
        }
        if current != marked
            && let Err(found) =
                state.compare_exchange_weak(current, marked, Ordering::Relaxed, Ordering::Relaxed)
        {
            current = found;
            continue;
        }
        native::wait(state, marked, None);
        current = state.load(Ordering::Relaxed);
    }
}

/// Acquire a lock whose inline compare-exchange failed.
///
/// # Safety
///
/// `state` is the live, aligned state word of a `Mutex` that no thread moves
/// or drops while any thread can lock it.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_mutex_lock_slow(state: *const AtomicU32) {
    // SAFETY: the caller's loans keep the mutex in place while it is locked.
    lock(unsafe { &*state });
}

/// `lock_now()` after the inline compare-exchange found only [`WAITING`]:
/// the mutex is unlocked while `lock_when` waiters sleep. Returns 1 if this
/// call acquired the lock. Like the inline attempt, it never waits and
/// records no lock waiter.
///
/// # Safety
///
/// As for [`plenty_mutex_lock_slow`].
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_mutex_try_lock(state: *const AtomicU32) -> u8 {
    // SAFETY: as above.
    let state = unsafe { &*state };
    let mut current = state.load(Ordering::Relaxed);
    while current & HELD == 0 {
        match state.compare_exchange_weak(
            current,
            current | 1,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            Ok(_) => return 1,
            Err(found) => current = found,
        }
    }
    0
}

/// Wake waiters after an unlock that replaced `previous`, a state above 1.
/// The unlock's exchange cleared [`WAITING`]; condition waiters that still
/// wait set it again before they sleep.
///
/// # Safety
///
/// As for [`plenty_mutex_lock_slow`]; `sequence` is the same mutex's sequence
/// word.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_mutex_release(
    state: *const AtomicU32,
    sequence: *const AtomicU32,
    previous: u32,
) {
    if previous & WAITING != 0 {
        // SAFETY: the caller keeps the sequence word live and aligned.
        let sequence = unsafe { &*sequence };
        sequence.fetch_add(1, Ordering::Release);
        native::wake(sequence, true);
    }
    if previous & HELD == CONTENDED {
        // SAFETY: the caller keeps the state word live and aligned.
        native::wake(unsafe { &*state }, false);
    }
}

/// One `lock_when` wait, called with the lock held. Returns 1 at once if the
/// monotonic clock has reached `deadline` nanoseconds. Otherwise reads the
/// sequence word, releases the lock without notifying other condition
/// waiters, sleeps until the sequence changes or at most [`SLICE`] passes,
/// relocks, and returns 0. Reading the sequence under the lock means an
/// unlock between the release and the sleep changes the word first, so the
/// sleep returns at once and no notification is lost. A return without one
/// is harmless: the caller rechecks its predicate under the lock.
///
/// # Safety
///
/// As for [`plenty_mutex_release`]. The calling thread holds the lock.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_mutex_wait(
    state: *const AtomicU32,
    sequence: *const AtomicU32,
    deadline: u64,
) -> u8 {
    // SAFETY: the caller keeps both words live and aligned while it waits.
    let (state, sequence) = unsafe { (&*state, &*sequence) };
    wait(state, sequence, deadline, SLICE)
}

/// [`plenty_mutex_wait`] with any longest sleep, so tests can make a lost
/// notification wait until the deadline instead of one slice.
fn wait(state: &AtomicU32, sequence: &AtomicU32, deadline: u64, slice: Duration) -> u8 {
    let now = crate::time::monotonic();
    if now >= deadline {
        return 1;
    }
    let observed = sequence.load(Ordering::Relaxed);
    if state.swap(WAITING, Ordering::Release) & HELD == CONTENDED {
        native::wake(state, false);
    }
    native::wait(
        sequence,
        observed,
        Some(Duration::from_nanos(deadline - now).min(slice)),
    );
    lock(state);
    0
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
        sequence: AtomicU32,
        value: UnsafeCell<(u64, u64)>,
    }

    impl Guarded {
        fn new() -> Self {
            Self {
                state: AtomicU32::new(0),
                sequence: AtomicU32::new(0),
                value: UnsafeCell::new((0, 0)),
            }
        }

        fn lock(&self) {
            if self
                .state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                // SAFETY: `self` outlives every thread in the test.
                unsafe { plenty_mutex_lock_slow(&self.state) };
            }
        }

        fn unlock(&self) {
            let previous = self.state.swap(0, Ordering::Release);
            if previous > 1 {
                // SAFETY: as above.
                unsafe { plenty_mutex_release(&self.state, &self.sequence, previous) };
            }
        }
    }

    // SAFETY: `value` is accessed only while `state` is locked.
    unsafe impl Sync for Guarded {}

    /// The same protocol generated code inlines around the runtime calls.
    fn locked(guarded: &Guarded, update: impl FnOnce(&mut (u64, u64))) {
        guarded.lock();
        // SAFETY: the lock is held.
        update(unsafe { &mut *guarded.value.get() });
        guarded.unlock();
    }

    /// The loop generated for `lock_when`, without cancellation: true once
    /// `ready` held under the lock, false at the deadline.
    fn locked_when(
        guarded: &Guarded,
        deadline: u64,
        slice: Duration,
        ready: impl Fn(&(u64, u64)) -> bool,
        update: impl FnOnce(&mut (u64, u64)),
    ) -> bool {
        guarded.lock();
        loop {
            // SAFETY: the lock is held.
            if ready(unsafe { &*guarded.value.get() }) {
                break;
            }
            if wait(&guarded.state, &guarded.sequence, deadline, slice) == 1 {
                guarded.unlock();
                return false;
            }
        }
        // SAFETY: the lock is held.
        update(unsafe { &mut *guarded.value.get() });
        guarded.unlock();
        true
    }

    fn after(duration: Duration) -> u64 {
        crate::time::monotonic() + duration.as_nanos() as u64
    }

    #[test]
    fn contended_updates_keep_both_fields_together() {
        const WORKERS: u64 = 8;
        const ROUNDS: u64 = 20_000;
        let guarded = Guarded::new();
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

        let guarded: [_; 4] = std::array::from_fn(|_| Guarded::new());
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
                                    backend::wait(&item.state, 2, None);
                                }
                            }
                            // SAFETY: this worker holds this item's mutex.
                            let value = unsafe { &mut *item.value.get() };
                            assert_eq!(value.0 * 2, value.1);
                            value.0 += 1;
                            std::hint::spin_loop();
                            value.1 += 2;
                            if item.state.swap(0, Ordering::Release) == 2 {
                                backend::wake(&item.state, false);
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

    #[cfg(unix)]
    #[test]
    fn pthread_parking_times_out_and_returns_when_the_word_changes() {
        #[cfg(target_vendor = "apple")]
        use super::native as backend;
        #[cfg(not(target_vendor = "apple"))]
        use super::pthread_test_backend as backend;

        // Every word shares one condition variable, so another test's wake
        // can end a sleep early; only returning without a wake is checked.
        let word = AtomicU32::new(0);
        let started = std::time::Instant::now();
        backend::wait(&word, 0, Some(Duration::from_millis(20)));
        // A word that no longer holds the expected value returns at once.
        backend::wait(&word, 1, None);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(20));
                word.store(1, Ordering::Relaxed);
                backend::wake(&word, true);
            });
            while word.load(Ordering::Relaxed) == 0 {
                backend::wait(&word, 0, Some(Duration::from_secs(60)));
            }
        });
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    /// Turns alternate between threads that each wait for their own number.
    /// Every handoff needs the unlock before it to wake the next waiter: with
    /// a slice as long as the deadline, a lost notification sleeps until the
    /// deadline and fails the test instead of recovering at the next slice.
    #[test]
    fn condition_waiters_see_every_change_made_under_the_lock() {
        const THREADS: u64 = 4;
        const ROUNDS: u64 = 500;
        let guarded = Guarded::new();
        let deadline = after(Duration::from_secs(60));
        std::thread::scope(|scope| {
            for me in 0..THREADS {
                let guarded = &guarded;
                scope.spawn(move || {
                    for _ in 0..ROUNDS {
                        let took = locked_when(
                            guarded,
                            deadline,
                            Duration::from_secs(60),
                            |(turn, _)| turn % THREADS == me,
                            |(turn, total)| {
                                *turn += 1;
                                *total += me;
                            },
                        );
                        assert!(took, "a waiter missed its turn");
                    }
                });
            }
        });
        let (turn, total) = guarded.value.into_inner();
        assert_eq!(turn, THREADS * ROUNDS);
        assert_eq!(total, ROUNDS * (0..THREADS).sum::<u64>());
        assert_eq!(guarded.state.into_inner() & HELD, 0);
    }

    #[test]
    fn a_wait_stops_at_its_deadline_and_keeps_the_lock_usable() {
        let guarded = Guarded::new();
        let started = std::time::Instant::now();
        let deadline = after(Duration::from_millis(30));
        assert!(!locked_when(&guarded, deadline, SLICE, |_| false, |_| {}));
        assert!(started.elapsed() >= Duration::from_millis(30));
        // An expired deadline returns before releasing the lock.
        assert!(!locked_when(&guarded, 0, SLICE, |_| false, |_| {}));
        assert!(locked_when(
            &guarded,
            0,
            SLICE,
            |_| true,
            |value| value.0 = 7
        ));
        assert_eq!(guarded.value.into_inner().0, 7);
        assert_eq!(guarded.state.into_inner(), 0);
    }

    #[test]
    fn lock_now_acquires_a_mutex_that_only_condition_waiters_mark() {
        let guarded = Guarded::new();
        guarded.state.store(WAITING, Ordering::Relaxed);
        // SAFETY: the words outlive the calls.
        assert_eq!(unsafe { plenty_mutex_try_lock(&guarded.state) }, 1);
        assert_eq!(guarded.state.load(Ordering::Relaxed), WAITING | 1);
        assert_eq!(unsafe { plenty_mutex_try_lock(&guarded.state) }, 0);
        guarded.unlock();
        assert_eq!(guarded.sequence.load(Ordering::Relaxed), 1);
        assert_eq!(guarded.state.load(Ordering::Relaxed), 0);
    }
}

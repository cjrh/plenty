//! Explicitly shared cooperative cancellation; dropping a handle does not cancel.
//!
//! A readiness wait (`std.ready`, `std.net`) also polls the token's wake
//! descriptor on Linux, an eventfd created on the first such wait and written
//! by `cancel`. Other platforms retain bounded polling for readiness; direct
//! cancellation waits use the condition variable on every platform.
use crate::aggregates;
use crate::deadline::Deadline;
use crate::memory::{self, AllocError, Header};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

#[repr(C)]
struct Token {
    header: Header,
    cancelled: AtomicBool,
    /// Also guards the wake descriptor, which `cancel` writes under the lock.
    gate: Mutex<Option<native::Event>>,
    changed: Condvar,
}

/// How a readiness wait observes a token.
pub(crate) enum Wake {
    /// Already cancelled: the wait must not block.
    Cancelled,
    /// Readable once the token is cancelled. It lives as long as the token.
    Descriptor(i32),
    /// No descriptor could be created (for example at the descriptor limit);
    /// the wait must block for at most one bounded slice and recheck.
    Unavailable,
}

/// The wake descriptor of a borrowed token, created on first use.
///
/// # Safety
/// `pointer` is a live token loan for the duration of the wait.
pub(crate) unsafe fn wake(pointer: u128) -> Wake {
    // SAFETY: the caller holds a live token loan.
    let token = unsafe { &*(pointer as *const Token) };
    let mut guard = token
        .gate
        .lock()
        .unwrap_or_else(|_| crate::fail("poisoned cancellation mutex"));
    if guard.is_none() {
        *guard = native::event();
    }
    // Checked under the lock: a cancel that ran before the descriptor existed
    // wrote nothing, so the flag is the only record of it.
    if token.is_cancelled() {
        return Wake::Cancelled;
    }
    match guard.as_ref() {
        Some(fd) => Wake::Descriptor(native::descriptor(fd)),
        None => Wake::Unavailable,
    }
}

#[cfg(target_os = "linux")]
mod native {
    use std::os::fd::{FromRawFd, OwnedFd};

    const EFD_NONBLOCK: i32 = 0o4000;
    const EFD_CLOEXEC: i32 = 0o2000000;

    unsafe extern "C" {
        fn eventfd(initial: u32, flags: i32) -> i32;
        fn write(fd: i32, data: *const std::ffi::c_void, length: usize) -> isize;
    }

    pub(super) type Event = OwnedFd;

    pub(super) fn descriptor(fd: &Event) -> i32 {
        std::os::fd::AsRawFd::as_raw_fd(fd)
    }

    pub(super) fn event() -> Option<OwnedFd> {
        // SAFETY: plain integer arguments; a fresh descriptor is owned here.
        let fd = unsafe { eventfd(0, EFD_NONBLOCK | EFD_CLOEXEC) };
        // SAFETY: nothing else owns a descriptor eventfd just returned.
        (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd) })
    }

    /// Makes the descriptor readable. The counter cannot overflow: it is
    /// written once and never read.
    pub(super) fn signal(fd: &OwnedFd) {
        let one = 1u64.to_ne_bytes();
        // SAFETY: an eight-byte write to a live eventfd.
        unsafe { write(std::os::fd::AsRawFd::as_raw_fd(fd), one.as_ptr().cast(), 8) };
    }
}

#[cfg(not(target_os = "linux"))]
mod native {
    // Readiness already falls back to bounded polling when no descriptor is
    // available. Keep that behavior on non-Linux targets without importing
    // Unix descriptor types on Windows or pretending to signal an event.
    pub(super) type Event = std::convert::Infallible;

    pub(super) fn event() -> Option<Event> {
        None
    }

    pub(super) fn descriptor(event: &Event) -> i32 {
        match *event {}
    }

    pub(super) fn signal(event: &Event) {
        match *event {}
    }
}

fn create() -> Result<*mut Token, AllocError> {
    let token = memory::try_allocate::<Token, u8>(0)?;
    // SAFETY: this is the uniquely owned, correctly aligned allocation.
    unsafe {
        token.write(Token {
            header: Header::new(destroy),
            cancelled: AtomicBool::new(false),
            gate: Mutex::new(None),
            changed: Condvar::new(),
        });
    }
    Ok(token)
}

impl Token {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        // Synchronize predicate changes with waiter registration. Polling does
        // not take the mutex; the release/acquire pair publishes cancellation.
        let guard = self
            .gate
            .lock()
            .unwrap_or_else(|_| crate::fail("poisoned cancellation mutex"));
        self.cancelled.store(true, Ordering::Release);
        if let Some(fd) = guard.as_ref() {
            native::signal(fd);
        }
        drop(guard);
        self.changed.notify_all();
    }

    fn wait(&self, deadline: Option<Deadline>) -> bool {
        let mut guard = self
            .gate
            .lock()
            .unwrap_or_else(|_| crate::fail("poisoned cancellation mutex"));
        loop {
            // Cancellation wins when it is visible as the deadline expires.
            if self.is_cancelled() {
                return true;
            }
            guard = match &deadline {
                Some(deadline) if deadline.expired() => return false,
                Some(deadline) => deadline.wait(&self.changed, guard),
                None => self
                    .changed
                    .wait(guard)
                    .unwrap_or_else(|_| crate::fail("poisoned cancellation mutex")),
            };
        }
    }
}

unsafe extern "C" fn destroy(header: *mut Header) {
    // SAFETY: each wait borrows a live ownership count. No waiter or caller
    // remains when the final release destroys the synchronization primitives.
    unsafe {
        let token = header.cast::<Token>();
        std::ptr::drop_in_place(token);
        memory::free::<Token, u8>(token, 0);
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_control(op: i64, args: *const u128, out: *mut u128) {
    // SAFETY: compiler-checked operations provide live token borrows, the exact
    // arity, and one output slot. The constructor returns its sole owned count.
    unsafe {
        let value = match op {
            0 => match create() {
                Ok(token) => aggregates::wrap(token as u128, 0),
                Err(error) => aggregates::wrap(aggregates::wrap(0, error as u64), 1),
            },
            1 => {
                (&*(*args as *const Token)).cancel();
                0
            }
            2 => (&*(*args as *const Token)).is_cancelled() as u128,
            3 => {
                (&*(*args as *const Token)).wait(None);
                0
            }
            4 => (&*(*args as *const Token)).wait(Some(Deadline::from_millis(*args.add(1) as u64)))
                as u128,
            _ => crate::fail("invalid cancellation operation"),
        };
        out.write(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{plenty_release, plenty_retain};
    use std::sync::Barrier;

    #[test]
    fn sharing_and_dropping_do_not_cancel() {
        unsafe {
            let token = create().unwrap();
            plenty_retain(token.cast());
            plenty_release(token.cast());
            assert!(!(*token).is_cancelled());
            assert!(!(*token).wait(Some(Deadline::from_millis(0))));
            assert!(!(*token).wait(Some(Deadline::from_millis(1))));
            (*token).cancel();
            (*token).cancel();
            assert!((*token).is_cancelled());
            assert!((*token).wait(Some(Deadline::from_millis(0))));
            assert!((*token).wait(Some(Deadline::from_millis(u64::MAX))));
            plenty_release(token.cast());
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn cancellation_retains_bounded_polling_without_a_descriptor() {
        // SAFETY: the token's owned reference stays live until final release.
        unsafe {
            let token = create().unwrap();
            assert!(matches!(wake(token as u128), Wake::Unavailable));
            (*token).cancel();
            assert!(matches!(wake(token as u128), Wake::Cancelled));
            plenty_release(token.cast());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cancellation_makes_the_wake_descriptor_readable() {
        #[repr(C)]
        struct PollFd {
            fd: i32,
            events: i16,
            revents: i16,
        }
        unsafe extern "C" {
            fn poll(fds: *mut PollFd, count: u64, timeout: i32) -> i32;
        }
        unsafe {
            let token = create().unwrap();
            let Wake::Descriptor(fd) = wake(token as u128) else {
                panic!("no wake descriptor");
            };
            // Later waits reuse the same descriptor.
            assert!(matches!(wake(token as u128), Wake::Descriptor(again) if again == fd));
            let mut entry = PollFd {
                fd,
                events: 1,
                revents: 0,
            };
            assert_eq!(poll(&mut entry, 1, 0), 0);
            let address = token.expose_provenance();
            let started = std::time::Instant::now();
            std::thread::scope(|scope| {
                scope.spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    (*std::ptr::with_exposed_provenance::<Token>(address)).cancel();
                });
                // One blocking poll with a long timeout ends on cancellation.
                assert_eq!(poll(&mut entry, 1, 10_000), 1);
            });
            assert!(started.elapsed() < std::time::Duration::from_secs(5));
            assert!(matches!(wake(token as u128), Wake::Cancelled));
            plenty_release(token.cast());
        }
    }

    #[test]
    fn cancellation_wakes_every_waiter_and_publishes_prior_writes() {
        let published = AtomicBool::new(false);
        let barrier = Barrier::new(5);
        unsafe {
            let token = create().unwrap();
            let address = token.expose_provenance();
            for _ in 0..4 {
                plenty_retain(token.cast());
            }
            std::thread::scope(|scope| {
                for index in 0..4 {
                    let published = &published;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        let token = std::ptr::with_exposed_provenance::<Token>(address);
                        barrier.wait();
                        let deadline = (index % 2 == 0).then(|| Deadline::from_millis(u64::MAX));
                        assert!((*token).wait(deadline));
                        assert!(published.load(Ordering::Relaxed));
                        plenty_release(token.cast_mut().cast());
                    });
                }
                barrier.wait();
                published.store(true, Ordering::Relaxed);
                (*token).cancel();
                plenty_release(token.cast());
            });
        }
    }
}

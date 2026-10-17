//! Termination signals as readable bytes, for `std.signal`.
//!
//! `listen` catches `SIGINT` and `SIGTERM` for the whole process. The handler
//! sends the signal's number as one byte on a socket pair whose other end the
//! program owns as a `File`, then restores the default action of both
//! signals, so a second signal terminates the process at once. If the
//! program has closed its end, the send fails and the handler re-raises the
//! signal under the default action: the process ends as if it had never
//! listened. See book/src/design/stdlib/signal.md.
use crate::aggregates::Type;
use crate::files;
use crate::text_io::Error;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use super::*;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;
    const CAUGHT: [i32; 2] = [SIGINT, SIGTERM];
    const SA_RESTART: i32 = 0x1000_0000;
    const SA_ONSTACK: i32 = 0x0800_0000;
    const AF_UNIX: i32 = 1;
    const SOCK_STREAM: i32 = 1;
    const SOCK_CLOEXEC: i32 = 0o2000000;
    const MSG_DONTWAIT: i32 = 0x40;
    const MSG_NOSIGNAL: i32 = 0x4000;
    const EBUSY: i32 = 16;
    const SIG_DFL: usize = 0;

    /// The glibc `struct sigaction` layout on x86_64 Linux.
    #[repr(C)]
    struct Action {
        handler: usize,
        mask: [u64; 16],
        flags: i32,
        restorer: usize,
    }

    extern "C" {
        fn sigaction(signal: i32, action: *const Action, previous: *mut Action) -> i32;
        fn socketpair(domain: i32, kind: i32, protocol: i32, pair: *mut i32) -> i32;
        fn send(fd: i32, data: *const u8, length: usize, flags: i32) -> isize;
        fn raise(signal: i32) -> i32;
        fn __errno_location() -> *mut i32;
    }

    /// The runtime's end of the pair, or -1. It is never closed once
    /// published, so the handler cannot send to a reused descriptor number.
    static WRITER: AtomicI32 = AtomicI32::new(-1);
    /// Set by the first `listen`; a process listens once.
    static CLAIMED: AtomicBool = AtomicBool::new(false);

    fn action(handler: usize, flags: i32) -> Action {
        Action {
            handler,
            mask: [0; 16],
            flags,
            restorer: 0,
        }
    }

    fn restore_defaults() {
        let default = action(SIG_DFL, 0);
        for signal in CAUGHT {
            // SAFETY: a valid record that outlives the call. sigaction is
            // async-signal-safe, so the handler may call this too.
            unsafe { sigaction(signal, &default, null_mut()) };
        }
    }

    /// Only async-signal-safe calls: send, sigaction and raise. errno is
    /// preserved for the interrupted code.
    extern "C" fn caught(signal: i32) {
        // SAFETY: errno is thread-local and valid for the thread's lifetime.
        let errno = unsafe { __errno_location() };
        // SAFETY: as above.
        let saved = unsafe { *errno };
        let byte = signal as u8;
        let fd = WRITER.load(Ordering::Acquire);
        // SAFETY: a one-byte send from a live local; a closed peer is an
        // error return, not SIGPIPE.
        let sent =
            fd >= 0 && unsafe { send(fd, &raw const byte, 1, MSG_DONTWAIT | MSG_NOSIGNAL) } == 1;
        restore_defaults();
        if !sent {
            // The signal is blocked while this handler runs, so it is
            // delivered under the default action as the handler returns.
            // SAFETY: plain integer argument.
            unsafe { raise(signal) };
        }
        // SAFETY: as above.
        unsafe { *errno = saved };
    }

    pub(super) fn listen(ty: &'static Type) -> Result<u128, Error> {
        if CLAIMED.swap(true, Ordering::AcqRel) {
            return Err(std::io::Error::from_raw_os_error(EBUSY).into());
        }
        let mut pair = [-1; 2];
        // SAFETY: `pair` has room for the two descriptors.
        if unsafe { socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, pair.as_mut_ptr()) } != 0 {
            let error = std::io::Error::last_os_error();
            CLAIMED.store(false, Ordering::Release);
            return Err(error.into());
        }
        // SAFETY: socketpair just created both descriptors; nothing else owns them.
        let (reader, writer) =
            unsafe { (OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])) };
        let file = match files::adopt_socket(std::fs::File::from(reader), ty) {
            Ok(file) => file,
            Err(error) => {
                // Nothing was published, so a later call may try again.
                drop(writer);
                CLAIMED.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let writer = std::os::fd::IntoRawFd::into_raw_fd(writer);
        WRITER.store(writer, Ordering::Release);
        let handled = action(caught as *const () as usize, SA_RESTART | SA_ONSTACK);
        for signal in CAUGHT {
            // SAFETY: a valid record that outlives the call.
            if unsafe { sigaction(signal, &handled, null_mut()) } != 0 {
                let error = std::io::Error::last_os_error();
                restore_defaults();
                // The writer stays open and published: a handler already
                // running may still hold its number. The claim stays too.
                // SAFETY: `file` is the owner `adopt_socket` just returned.
                unsafe { crate::memory::plenty_release(file as *mut crate::memory::Header) };
                return Err(error.into());
            }
        }
        Ok(file)
    }

    const _: () = assert!(size_of::<Action>() == 152);
}

/// Starts catching `SIGINT` and `SIGTERM` and returns the readable end on
/// which each caught signal arrives as one byte. A second call fails with
/// `EBUSY`.
pub(crate) fn listen(ty: &'static Type) -> Result<u128, Error> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        native::listen(ty)
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = (ty, files::adopt_socket);
        Err(crate::text_io::Kind::Unsupported.into())
    }
}

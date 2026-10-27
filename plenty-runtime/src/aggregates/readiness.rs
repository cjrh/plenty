//! Readiness waits behind `std.ready` that a cancellation token ends at once,
//! and selectors that wait on many handles in one call.
//!
//! A cancellable wait polls the handle together with the token's wake
//! descriptor (see `control::wake`), so `cancel()` wakes it directly. A
//! selector is an epoll instance owned by a `File`. Registrations are
//! level-triggered and keyed by a caller-chosen `u64`; closing a registered
//! handle removes it. A selector wait polls the epoll descriptor with the
//! token's wake descriptor, then collects ready events without blocking.
//! Deadlines, retries and event decoding are checked Plenty in std.ready.
use super::Type;
use crate::text_io::{self, Error, Kind};

/// Bytes per event in a selector's event buffer: the key as eight
/// little-endian bytes, the readiness bits, then seven zero bytes.
const EVENT_SIZE: usize = 16;

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod native {
    use super::super::byte_storage::Storage;
    use super::*;
    use crate::control::{Wake, wake};
    use std::os::fd::{FromRawFd, OwnedFd};

    /// The longest a wait blocks when the token has no wake descriptor.
    const SLICE: u64 = 10_000_000;
    /// Events collected per selector wait; more stay ready for the next one.
    const BATCH: usize = 64;

    const POLLIN: i16 = 0x1;
    const POLLOUT: i16 = 0x4;
    const POLLERR: i16 = 0x8;
    const POLLHUP: i16 = 0x10;
    const POLLNVAL: i16 = 0x20;

    const EPOLL_CLOEXEC: i32 = 0o2000000;
    const EPOLL_CTL_ADD: i32 = 1;
    const EPOLL_CTL_DEL: i32 = 2;
    const EPOLL_CTL_MOD: i32 = 3;
    const EPOLLIN: u32 = 0x1;
    const EPOLLOUT: u32 = 0x4;
    const EPOLLERR: u32 = 0x8;
    const EPOLLHUP: u32 = 0x10;
    const EEXIST: i32 = 17;
    const ENOENT: i32 = 2;

    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanoseconds: i64,
    }

    /// The kernel packs this structure on x86_64 only.
    #[cfg_attr(target_arch = "x86_64", repr(C, packed))]
    #[cfg_attr(not(target_arch = "x86_64"), repr(C))]
    #[derive(Clone, Copy)]
    struct EpollEvent {
        events: u32,
        data: u64,
    }

    unsafe extern "C" {
        fn ppoll(
            fds: *mut PollFd,
            count: u64,
            timeout: *const Timespec,
            mask: *const std::ffi::c_void,
        ) -> i32;
        fn epoll_create1(flags: i32) -> i32;
        fn epoll_ctl(epoll: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32;
        fn epoll_wait(epoll: i32, events: *mut EpollEvent, count: i32, timeout: i32) -> i32;
    }

    fn checked(status: i32) -> Result<i32, Error> {
        if status < 0 {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(status)
        }
    }

    fn interest(bits: u8) -> Result<i16, Error> {
        if bits == 0 || bits > 3 {
            return Err(Kind::InvalidInput.into());
        }
        Ok(if bits & 1 != 0 { POLLIN } else { 0 } | if bits & 2 != 0 { POLLOUT } else { 0 })
    }

    /// Bit 0 readable, bit 1 writable, bit 2 hang-up, bit 3 error.
    fn reported(revents: i16) -> u128 {
        [(POLLIN, 1), (POLLOUT, 2), (POLLHUP, 4), (POLLERR, 8)]
            .into_iter()
            .filter(|(flag, _)| revents & flag != 0)
            .fold(0, |bits, (_, bit)| bits | bit)
    }

    /// One `ppoll` on `fd` and, when the token has one, its wake descriptor.
    /// Returns `fd`'s revents; zero means the timeout elapsed or the token
    /// was cancelled. An interrupted poll is the error EINTR.
    unsafe fn poll(fd: i32, events: i16, timeout: u64, token: u128) -> Result<i16, Error> {
        // SAFETY: the caller holds a live token loan.
        let (wake_fd, timeout) = match unsafe { wake(token) } {
            Wake::Cancelled => return Ok(0),
            Wake::Descriptor(wake_fd) => (wake_fd, timeout),
            Wake::Unavailable => (-1, timeout.min(SLICE)),
        };
        // A negative descriptor is ignored by the kernel.
        let mut entries = [
            PollFd {
                fd,
                events,
                revents: 0,
            },
            PollFd {
                fd: wake_fd,
                events: POLLIN,
                revents: 0,
            },
        ];
        let span = Timespec {
            seconds: (timeout / 1_000_000_000) as i64,
            nanoseconds: (timeout % 1_000_000_000) as i64,
        };
        // SAFETY: two initialized entries and a valid relative timeout; a
        // null mask leaves the thread's signal mask unchanged.
        checked(unsafe { ppoll(entries.as_mut_ptr(), 2, &span, std::ptr::null()) })?;
        if entries[0].revents & POLLNVAL != 0 {
            return Err(Kind::Closed.into());
        }
        Ok(entries[0].revents)
    }

    unsafe fn wait(file: u128, bits: u8, timeout: u64, token: u128) -> Result<u128, Error> {
        let events = interest(bits)?;
        // SAFETY: compiler-checked loans of a file and a token.
        unsafe {
            let fd = crate::files::descriptor(file as *const _)?;
            Ok(reported(poll(fd, events, timeout, token)?))
        }
    }

    fn open(ty: &'static Type) -> Result<u128, Error> {
        // SAFETY: an integer flag; the fresh descriptor is owned here only.
        let fd = checked(unsafe { epoll_create1(EPOLL_CLOEXEC) })?;
        // SAFETY: nothing else owns the descriptor epoll_create1 returned.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        crate::files::adopt_socket(std::fs::File::from(owned), ty)
    }

    /// Adds or changes `target`'s registration; interest 0 removes it.
    /// Removing a handle that is not registered succeeds.
    fn control(selector: i32, target: i32, key: u64, bits: u8) -> Result<u128, Error> {
        let mut event = EpollEvent {
            events: 0,
            data: key,
        };
        if bits == 0 {
            // SAFETY: live descriptors; the event is ignored for removal.
            if unsafe { epoll_ctl(selector, EPOLL_CTL_DEL, target, &mut event) } < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(ENOENT) {
                    return Err(error.into());
                }
            }
            return Ok(0);
        }
        let events = interest(bits)?;
        event.events = if events & POLLIN != 0 { EPOLLIN } else { 0 }
            | if events & POLLOUT != 0 { EPOLLOUT } else { 0 };
        // SAFETY: live descriptors and an initialized event.
        if unsafe { epoll_ctl(selector, EPOLL_CTL_ADD, target, &mut event) } < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(EEXIST) {
                return Err(error.into());
            }
            // SAFETY: as above, for a registered descriptor.
            checked(unsafe { epoll_ctl(selector, EPOLL_CTL_MOD, target, &mut event) })?;
        }
        Ok(0)
    }

    /// Waits until a registered handle is ready, the timeout elapses or the
    /// token is cancelled, then writes up to `out.len() / 16` events.
    unsafe fn select(
        out: &mut [u8],
        selector: i32,
        timeout: u64,
        token: u128,
    ) -> Result<u128, Error> {
        let capacity = (out.len() / EVENT_SIZE).min(BATCH);
        if capacity == 0 {
            return Err(Kind::InvalidInput.into());
        }
        // SAFETY: the caller holds a live token loan.
        if unsafe { poll(selector, POLLIN, timeout, token)? } & POLLIN == 0 {
            return Ok(0);
        }
        let mut events = [EpollEvent { events: 0, data: 0 }; BATCH];
        // SAFETY: room for `capacity` events; a zero timeout does not block.
        let count =
            checked(unsafe { epoll_wait(selector, events.as_mut_ptr(), capacity as i32, 0) })?;
        for (event, slot) in events[..count as usize]
            .iter()
            .zip(out.as_chunks_mut::<EVENT_SIZE>().0)
        {
            let (flags, key) = (event.events, event.data);
            let mut bits = 0u8;
            for (flag, bit) in [(EPOLLIN, 1), (EPOLLOUT, 2), (EPOLLHUP, 4), (EPOLLERR, 8)] {
                if flags & flag != 0 {
                    bits |= bit;
                }
            }
            slot[..8].copy_from_slice(&key.to_le_bytes());
            slot[8] = bits;
            slot[9..].fill(0);
        }
        Ok(count as u128)
    }

    // Inlined so each operation's entry symbol keeps only its own arm.
    #[inline(always)]
    pub(super) unsafe fn call(
        op: i64,
        operands: [u128; 5],
        descriptor: &'static Type,
    ) -> Result<u128, Error> {
        let [a, b, c, d, _] = operands;
        let file = |value: u128| unsafe { crate::files::descriptor(value as *const _) };
        // SAFETY: compiler signatures check each operand; files, tokens and
        // byte storage are live loans for the duration of the call.
        unsafe {
            match op {
                419 => wait(a, b as u8, c as u64, d),
                420 => open(descriptor.variants[0].fields[0]),
                421 => control(file(a)?, file(b)?, c as u64, d as u8),
                422 => {
                    let selector = file(b)?;
                    let out = &mut (*(a as *mut Storage)).bytes;
                    select(out, selector, c as u64, d)
                }
                423 => Ok(file(a)? as u128),
                _ => crate::fail("invalid readiness operation"),
            }
        }
    }
}

/// Operands are live values of the statically checked signature.
// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(super) unsafe fn call(op: i64, operands: [u128; 5], descriptor: *const Type) -> u128 {
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    {
        // SAFETY: codegen supplies process-lifetime output metadata.
        text_io::result(unsafe { native::call(op, operands, &*descriptor) })
    }
    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    {
        let _ = (op, operands, descriptor, EVENT_SIZE);
        text_io::result(Err(Kind::Unsupported.into()))
    }
}

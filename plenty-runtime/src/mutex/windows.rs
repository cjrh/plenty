//! Windows address waits follow the same compare-before-sleep contract as
//! Linux futexes. See Rust library/std/src/sys/sync/futex/windows.rs.
use super::*;
use std::ffi::c_void;

// MSVC and MinGW name this Windows 8+ API import library differently.
#[cfg_attr(target_env = "msvc", link(name = "synchronization"))]
#[cfg_attr(not(target_env = "msvc"), link(name = "api-ms-win-core-synch-l1-2-0"))]
unsafe extern "system" {
    fn WaitOnAddress(
        address: *const c_void,
        compare: *const c_void,
        size: usize,
        timeout: u32,
    ) -> i32;
    fn WakeByAddressSingle(address: *const c_void);
    fn WakeByAddressAll(address: *const c_void);
}

/// Sleep while `word` holds `expected`, at most `timeout`.
pub(super) fn wait(word: &AtomicU32, expected: u32, timeout: Option<Duration>) {
    // Whole milliseconds, rounded up so a short slice still sleeps; INFINITE
    // is u32::MAX.
    let milliseconds = timeout.map_or(u32::MAX, |timeout| {
        timeout
            .as_nanos()
            .div_ceil(1_000_000)
            .min(u128::from(u32::MAX - 1)) as u32
    });
    // SAFETY: the caller keeps the aligned four-byte word live, and the
    // comparison value is live for this call.
    let woken = unsafe {
        WaitOnAddress(
            word.as_ptr().cast(),
            (&raw const expected).cast(),
            size_of::<u32>(),
            milliseconds,
        )
    };
    // A timed wait reports its timeout as failure; the caller rechecks.
    if woken == 0 && timeout.is_none() {
        crate::fail("mutex address wait failed");
    }
}

pub(super) fn wake(word: &AtomicU32, all: bool) {
    // SAFETY: the word remains live; the OS uses its address as a wait key.
    unsafe {
        if all {
            WakeByAddressAll(word.as_ptr().cast());
        } else {
            WakeByAddressSingle(word.as_ptr().cast());
        }
    }
}

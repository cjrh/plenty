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
}

pub(super) fn wait(state: &AtomicU32) {
    let expected = 2_u32;
    // SAFETY: the caller keeps the aligned four-byte word live. The comparison
    // value is live for this call, and INFINITE cannot produce a timeout.
    if unsafe {
        WaitOnAddress(
            state.as_ptr().cast(),
            (&raw const expected).cast(),
            size_of::<u32>(),
            u32::MAX,
        )
    } == 0
    {
        crate::fail("mutex address wait failed");
    }
}

pub(super) fn wake(state: &AtomicU32) {
    // SAFETY: the word remains live; the OS uses its address as a wait key.
    unsafe { WakeByAddressSingle(state.as_ptr().cast()) };
}

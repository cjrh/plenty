//! Narrow native-call helpers. Never allocate or run cleanup before errno capture.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_ffi_errno() -> i32 {
    // SAFETY: each CRT accessor returns this calling thread's live errno slot.
    unsafe {
        #[cfg(target_os = "linux")]
        {
            *libc::__errno_location()
        }
        #[cfg(target_os = "macos")]
        {
            *libc::__error()
        }
        #[cfg(target_os = "windows")]
        {
            unsafe extern "C" {
                fn _errno() -> *mut i32;
            }
            *_errno()
        }
    }
}

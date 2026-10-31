//! The OS entropy primitive behind `std.secrets`: one native request
//! (`getrandom` on Linux, `getentropy` on Apple, `BCryptGenRandom` on Windows)
//! into a caller-owned range. A short count is success and an interrupted
//! call surfaces as its OS error, so retry policy stays in checked Plenty code.
//! There is no fallback source: a failed call never yields weaker bytes.
use std::io;

#[cfg(target_os = "linux")]
type Source = unsafe extern "C" fn(*mut u8, usize, u32) -> isize;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn getrandom(buffer: *mut u8, length: usize, flags: u32) -> isize;
}

#[cfg(target_os = "linux")]
pub(crate) fn read(target: &mut [u8]) -> io::Result<u128> {
    // Flags 0 selects the urandom pool and blocks until it is initialized.
    read_with(getrandom, target)
}

#[cfg(target_vendor = "apple")]
pub(crate) fn read(target: &mut [u8]) -> io::Result<u128> {
    // getentropy accepts at most 256 bytes. Return that progress and let the
    // caller choose whether to repeat, exactly as for a short Linux read.
    let count = target.len().min(256);
    if count == 0 {
        return Ok(0);
    }
    // SAFETY: the slice owns count writable bytes; the OS fills all of them on
    // success. getentropy does not use a user-space pseudorandom fallback.
    if unsafe { libc::getentropy(target.as_mut_ptr().cast(), count) } == 0 {
        Ok(count as u128)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
pub(crate) fn read(target: &mut [u8]) -> io::Result<u128> {
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut std::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlNtStatusToDosError(status: i32) -> u32;
    }
    let count = target.len().min(u32::MAX as usize) as u32;
    if count == 0 {
        return Ok(0);
    }
    // SAFETY: the caller owns count writable bytes. A null algorithm with
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG selects the OS cryptographic generator.
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), target.as_mut_ptr(), count, 2) };
    if status >= 0 {
        Ok(count as u128)
    } else {
        // BCrypt returns NTSTATUS, not GetLastError. Translate it before
        // exposing an io::Error so std.io receives the actual Windows code.
        // SAFETY: translation accepts any NTSTATUS value and owns no pointers.
        let code = unsafe { RtlNtStatusToDosError(status) };
        Err(io::Error::from_raw_os_error(code as i32))
    }
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
pub(crate) fn read(_target: &mut [u8]) -> io::Result<u128> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(target_os = "linux")]
fn read_with(source: Source, target: &mut [u8]) -> io::Result<u128> {
    if target.is_empty() {
        return Ok(0);
    }
    // SAFETY: the pointer and length describe one exclusively borrowed slice.
    let count = unsafe { source(target.as_mut_ptr(), target.len(), 0) };
    match usize::try_from(count) {
        Ok(count) if count <= target.len() => Ok(count as u128),
        // A count beyond the request cannot be trusted as progress.
        Ok(_) => Err(io::ErrorKind::Other.into()),
        Err(_) => Err(io::Error::last_os_error()),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn __errno_location() -> *mut i32;
    }

    unsafe extern "C" fn short(buffer: *mut u8, length: usize, _flags: u32) -> isize {
        let count = length.min(3);
        // SAFETY: `count` bytes lie within the caller's slice.
        unsafe { std::ptr::write_bytes(buffer, 0xA5, count) };
        count as isize
    }

    unsafe extern "C" fn interrupted(_buffer: *mut u8, _length: usize, _flags: u32) -> isize {
        // SAFETY: errno is this thread's own slot.
        unsafe { *__errno_location() = 4 };
        -1
    }

    unsafe extern "C" fn unavailable(_buffer: *mut u8, _length: usize, _flags: u32) -> isize {
        // SAFETY: errno is this thread's own slot.
        unsafe { *__errno_location() = 38 };
        -1
    }

    unsafe extern "C" fn excessive(_buffer: *mut u8, length: usize, _flags: u32) -> isize {
        length as isize + 1
    }

    unsafe extern "C" fn untouched(_buffer: *mut u8, _length: usize, _flags: u32) -> isize {
        panic!("an empty range must not reach the OS")
    }

    #[test]
    fn short_read_reports_only_the_bytes_written() {
        let mut target = [0u8; 8];
        assert_eq!(read_with(short, &mut target).unwrap(), 3);
        assert_eq!(target, [0xA5, 0xA5, 0xA5, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn failures_keep_their_os_code_and_write_nothing() {
        for (source, code) in [(interrupted as Source, 4), (unavailable as Source, 38)] {
            let mut target = [7u8; 4];
            let error = read_with(source, &mut target).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(code));
            assert_eq!(target, [7; 4]);
        }
    }

    #[test]
    fn impossible_progress_is_an_error() {
        let mut target = [0u8; 4];
        let error = read_with(excessive, &mut target).unwrap_err();
        assert_eq!(error.raw_os_error(), None);
    }

    #[test]
    fn empty_range_makes_no_call() {
        assert_eq!(read_with(untouched, &mut []).unwrap(), 0);
    }
}

#[cfg(all(test, any(target_os = "linux", target_vendor = "apple", windows)))]
mod native_tests {
    use super::*;

    #[test]
    fn os_source_fills_the_whole_small_request() {
        // Each native backend fills small requests completely.
        let mut first = [0u8; 32];
        let mut second = [0u8; 32];
        assert_eq!(read(&mut first).unwrap(), 32);
        assert_eq!(read(&mut second).unwrap(), 32);
        assert_ne!(first, second);
    }

    #[test]
    fn empty_range_returns_zero() {
        assert_eq!(read(&mut []).unwrap(), 0);
    }

    #[test]
    fn large_request_reports_progress_and_leaves_the_tail_untouched() {
        let mut target = [0xa5; 1024];
        let count = read(&mut target).unwrap() as usize;
        assert!(count > 0 && count <= target.len());
        #[cfg(target_vendor = "apple")]
        assert_eq!(count, 256);
        assert!(target[..count].iter().any(|byte| *byte != 0xa5));
        assert!(target[count..].iter().all(|byte| *byte == 0xa5));
    }
}

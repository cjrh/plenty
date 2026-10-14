//! The one OS entropy primitive behind `std.secrets`: a single `getrandom(2)`
//! call into a caller-owned range. A short count is success and an interrupted
//! call surfaces as its OS error, so retry policy stays in checked Plenty code.
//! There is no fallback source: a failed call never yields weaker bytes.
use std::io;

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

#[cfg(not(target_os = "linux"))]
pub(crate) fn read(_target: &mut [u8]) -> io::Result<u128> {
    Err(io::ErrorKind::Unsupported.into())
}

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

    #[test]
    fn os_source_fills_the_whole_small_request() {
        // getrandom never returns short for requests up to 256 bytes.
        let mut first = [0u8; 32];
        let mut second = [0u8; 32];
        assert_eq!(read(&mut first).unwrap(), 32);
        assert_eq!(read(&mut second).unwrap(), 32);
        assert_ne!(first, second);
    }
}

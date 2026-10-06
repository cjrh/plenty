//! Fallible text I/O. No infallible growing buffers or allocated diagnostics.
use crate::{aggregates::wrap, memory::AllocError, strings};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

static ARG_COUNT: AtomicUsize = AtomicUsize::new(0);
static ARG_VECTOR: AtomicPtr<*const u8> = AtomicPtr::new(std::ptr::null_mut());

#[cfg(plenty_runtime_embedded)]
pub(crate) unsafe fn set_arguments(argc: i32, argv: *const *const u8) {
    ARG_VECTOR.store(argv.cast_mut(), Ordering::Relaxed);
    ARG_COUNT.store(argc.max(0) as usize, Ordering::Release);
}

pub(crate) unsafe fn arguments(ty: &'static crate::aggregates::Type) -> Result<u128, Error> {
    let count = ARG_COUNT.load(Ordering::Acquire);
    let vector = ARG_VECTOR.load(Ordering::Relaxed);
    // SAFETY: startup publishes argc pointers to immutable process-lifetime
    // NUL-terminated arguments before any Plenty code can request a snapshot.
    unsafe {
        let texts = (0..count).map(|index| {
            std::ffi::CStr::from_ptr((*vector.add(index)).cast())
                .to_str()
                .map_err(|_| Error::InvalidUtf8)
        });
        crate::aggregates::try_text_list(texts, ty)
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    System(std::io::Error),
    Allocation(AllocError),
    InvalidUtf8,
}
impl From<AllocError> for Error {
    fn from(error: AllocError) -> Self {
        Self::Allocation(error)
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::System(error)
    }
}

pub(crate) fn result(value: Result<u128, Error>) -> u128 {
    match value {
        Ok(value) => wrap(value, 0),
        Err(error) => {
            let error = match error {
                Error::System(error) => crate::io::system_error(error),
                Error::InvalidUtf8 => wrap(wrap(0, 0), 1),
                Error::Allocation(error) => wrap(wrap(wrap(0, error as u64), 1), 1),
            };
            wrap(error, 1)
        }
    }
}

fn push(bytes: &mut Vec<u8>, byte: u8) -> Result<(), Error> {
    if bytes.len() == i64::MAX as usize {
        return Err(AllocError::CapacityOverflow.into());
    }
    bytes.try_reserve(1).map_err(|_| AllocError::OutOfMemory)?;
    bytes.push(byte);
    Ok(())
}

fn open_file(path: &str, write: bool) -> Result<std::fs::File, Error> {
    if path.as_bytes().contains(&0) {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::FromRawFd;
        let length = path
            .len()
            .checked_add(1)
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or(AllocError::CapacityOverflow)?;
        let mut name = Vec::new();
        name.try_reserve_exact(length)
            .map_err(|_| AllocError::OutOfMemory)?;
        name.extend_from_slice(path.as_bytes());
        name.push(0);
        unsafe extern "C" {
            fn open(path: *const std::ffi::c_char, flags: i32, ...) -> i32;
        }
        // SAFETY: the checked fallible buffer is NUL terminated. O_CLOEXEC
        // prevents leaking this private descriptor to a future child process.
        let flags = 0o2000000 | if write { 0o1 | 0o100 | 0o1000 } else { 0 };
        let fd = unsafe { open(name.as_ptr().cast(), flags, 0o666u32) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: open returned a new descriptor; File takes sole ownership.
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = write;
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

impl From<std::io::ErrorKind> for Error {
    fn from(kind: std::io::ErrorKind) -> Self {
        Self::System(kind.into())
    }
}

fn read_all(reader: &mut impl Read) -> Result<*mut strings::Text, Error> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let count = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        bytes
            .len()
            .checked_add(count)
            .filter(|n| *n <= i64::MAX as usize)
            .ok_or(AllocError::CapacityOverflow)?;
        bytes
            .try_reserve(count)
            .map_err(|_| AllocError::OutOfMemory)?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    // Python-style universal text newlines, compacted without another allocation.
    let (mut read, mut written) = (0, 0);
    while read < bytes.len() {
        let mut byte = bytes[read];
        read += 1;
        if byte == b'\r' {
            byte = b'\n';
            if bytes.get(read) == Some(&b'\n') {
                read += 1;
            }
        }
        bytes[written] = byte;
        written += 1;
    }
    bytes.truncate(written);
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::InvalidUtf8)?;
    Ok(strings::try_new(text)?)
}

pub(crate) fn read_text(path: &str) -> Result<u128, Error> {
    let mut file = open_file(path, false)?;
    Ok(read_all(&mut file)? as u128)
}

pub(crate) fn write_text(path: &str, text: &str) -> Result<u128, Error> {
    let mut file = open_file(path, true)?;
    file.write_all(text.as_bytes())?;
    close(file)?;
    Ok(text.chars().count() as u128)
}

fn close(file: std::fs::File) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::os::fd::IntoRawFd;
        unsafe extern "C" {
            fn close(fd: i32) -> i32;
        }
        let fd = file.into_raw_fd();
        // SAFETY: consume our sole descriptor exactly once. Do not retry close
        // after EINTR: on Linux it has already relinquished the descriptor.
        if unsafe { close(fd) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    #[cfg(not(unix))]
    drop(file);
    Ok(())
}

fn line(reader: &mut impl Read) -> Result<u128, Error> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0];
        match reader.read(&mut byte) {
            Ok(0) if bytes.is_empty() => return Ok(wrap(0, 0)),
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => {
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                break;
            }
            Ok(_) => push(&mut bytes, byte[0])?,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::InvalidUtf8)?;
    Ok(wrap(strings::try_new(text)? as u128, 1))
}

struct Stdin;
impl Read for Stdin {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        #[cfg(unix)]
        {
            unsafe extern "C" {
                fn read(fd: i32, buffer: *mut std::ffi::c_void, count: usize) -> isize;
            }
            // SAFETY: the writable slice is valid for its length. The process
            // owns fd 0; a closed descriptor is an ordinary OS error. No owning
            // File or infallibly allocated standard-input buffer is constructed.
            let count = unsafe { read(0, bytes.as_mut_ptr().cast(), bytes.len()) };
            if count < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(count as usize)
            }
        }
        #[cfg(not(unix))]
        {
            let _ = bytes;
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}

pub(crate) fn input() -> u128 {
    result(line(&mut Stdin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{plenty_release, Header};
    #[test]
    fn complete_reads_translate_newlines_across_chunks() {
        let mut input = vec![b'x'; 8191];
        input.extend_from_slice(b"\r\n\r\0\n");
        let text = read_all(&mut input.as_slice()).unwrap();
        unsafe {
            let content = strings::utf8(text);
            assert_eq!(content.len(), 8195);
            assert!(content.ends_with("\n\n\0\n"));
            plenty_release(text.cast());
        }
        assert!(matches!(
            read_all(&mut &b"\xff"[..]),
            Err(Error::InvalidUtf8)
        ));
    }
    #[test]
    fn lines_do_not_read_ahead_and_preserve_nul_and_utf8() {
        let mut source = "é\0\r\n\nlast".as_bytes();
        for expected in ["é\0", "", "last"] {
            let value = line(&mut source).unwrap();
            assert_eq!(value >> 64, 1);
            unsafe {
                let text = value as u64 as *const strings::Text;
                assert_eq!(strings::utf8(text), expected);
                plenty_release(text.cast_mut().cast::<Header>());
            }
        }
        assert_eq!(line(&mut source).unwrap(), 0);
        assert!(matches!(line(&mut &b"\xff\n"[..]), Err(Error::InvalidUtf8)));
    }
}

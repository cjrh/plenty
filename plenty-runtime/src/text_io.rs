//! Fallible text I/O. No infallible growing buffers or allocated diagnostics.
use crate::{aggregates::wrap, memory::AllocError, strings};
use std::io::Read;

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

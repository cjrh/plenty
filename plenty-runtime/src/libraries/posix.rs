//! POSIX loader flags come from libc's target-specific declarations. Darwin
//! supplies dlopen through libSystem and does not have a separate libdl.
use super::*;
use libc::{dlclose, dlerror, dlopen, dlsym};

struct Terminated {
    pointer: *mut u8,
    len: usize,
}

impl Terminated {
    fn new(bytes: &[u8]) -> Result<Self, LoadError> {
        let pointer = memory::try_allocate::<u8, u8>(bytes.len())?;
        // SAFETY: the one-byte header plus len-byte tail has room for NUL.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
        Ok(Self {
            pointer,
            len: bytes.len(),
        })
    }
}

impl Drop for Terminated {
    fn drop(&mut self) {
        // SAFETY: this unique buffer still has its original allocation layout.
        unsafe { memory::free::<u8, u8>(self.pointer, self.len) };
    }
}

pub(super) fn open(path: &[u8]) -> Result<*mut c_void, LoadError> {
    let path = Terminated::new(path)?;
    // SAFETY: the allocation is NUL terminated. LOCAL avoids exporting symbols,
    // NOW resolves relocations, and NODELETE retains code after the lease closes.
    let handle = unsafe {
        dlopen(
            path.pointer.cast(),
            libc::RTLD_NOW | libc::RTLD_LOCAL | libc::RTLD_NODELETE,
        )
    };
    if handle.is_null() {
        Err(LoadError::OpenFailed)
    } else {
        Ok(handle)
    }
}

pub(super) unsafe fn symbol(
    handle: *mut c_void,
    name: *const u8,
) -> Result<*mut c_void, LoadError> {
    // SAFETY: the caller supplies a live handle and NUL-terminated symbol.
    unsafe {
        dlerror();
        let address = dlsym(handle, name.cast());
        if !dlerror().is_null() || address.is_null() {
            Err(LoadError::MissingSymbol)
        } else {
            Ok(address)
        }
    }
}

pub(super) unsafe fn close(handle: *mut c_void) {
    // SAFETY: the caller owns exactly one live lookup lease.
    unsafe { dlclose(handle) };
}

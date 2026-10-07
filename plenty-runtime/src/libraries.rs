//! GNU/Linux resident libraries. Lookup leases close; resolved code stays mapped.
use crate::memory::{self, AllocError};
use std::ffi::{c_char, c_int, c_void};
use std::ptr;

#[link(name = "dl")]
extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

// glibc's dlfcn.h on the compiler's supported x86_64 Linux GNU target.
const RTLD_NOW: c_int = 2;
const RTLD_NODELETE: c_int = 0x1000;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadError {
    OutOfMemory = 1,
    CapacityOverflow = 2,
    InvalidPath = 3,
    OpenFailed = 4,
    InvalidSymbol = 5,
    MissingSymbol = 6,
    IncompatibleContract = 7,
}

impl From<AllocError> for LoadError {
    fn from(error: AllocError) -> Self {
        match error {
            AllocError::OutOfMemory => Self::OutOfMemory,
            AllocError::CapacityOverflow => Self::CapacityOverflow,
        }
    }
}

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

fn open(path: &[u8]) -> Result<*mut c_void, LoadError> {
    if path.is_empty() || path.contains(&0) {
        return Err(LoadError::InvalidPath);
    }
    let path = Terminated::new(path)?;
    // SAFETY: the path is terminated; NOW resolves relocations before returning.
    // LOCAL is zero. NODELETE keeps successful mappings resident after dlclose.
    let handle = unsafe { dlopen(path.pointer.cast(), RTLD_NOW | RTLD_NODELETE) };
    if handle.is_null() {
        Err(LoadError::OpenFailed)
    } else {
        Ok(handle)
    }
}

unsafe fn symbol(handle: *mut c_void, name: &[u8]) -> Result<*mut c_void, LoadError> {
    // Bound names so lookup itself needs no allocation, including on failure.
    let valid = !name.is_empty()
        && name.len() < 512
        && (name[0].is_ascii_alphabetic() || name[0] == b'_')
        && name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_');
    if !valid {
        return Err(LoadError::InvalidSymbol);
    }
    if handle.is_null() {
        return Err(LoadError::MissingSymbol);
    }
    let mut terminated = [0u8; 512];
    terminated[..name.len()].copy_from_slice(name);
    // SAFETY: caller owns a live lookup lease; name is NUL-terminated. Clear
    // prior loader errors before testing this lookup. Null is never callable.
    unsafe {
        dlerror();
        let address = dlsym(handle, terminated.as_ptr().cast());
        if !dlerror().is_null() || address.is_null() {
            Err(LoadError::MissingSymbol)
        } else {
            Ok(address)
        }
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_library_open_v1(
    path: *const u8,
    len: usize,
    output: *mut *mut c_void,
) -> u32 {
    // SAFETY: generated wrappers supply a live byte slice and disjoint output.
    match open(unsafe { std::slice::from_raw_parts(path, len) }) {
        Ok(handle) => {
            unsafe { output.write(handle) };
            0
        }
        Err(error) => error as u32,
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_library_symbol_v1(
    handle: *mut c_void,
    name: *const u8,
    len: usize,
    output: *mut *mut c_void,
) -> u32 {
    // SAFETY: wrappers keep the lease, input, and output live across the call.
    match unsafe { symbol(handle, std::slice::from_raw_parts(name, len)) } {
        Ok(address) => {
            unsafe { output.write(address) };
            0
        }
        Err(error) => error as u32,
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_library_close_v1(handle: *mut c_void) {
    // SAFETY: exactly one matching successful open, or an empty wrapper. This
    // releases the lookup lease, never unmaps NODELETE code or its static state.
    if !handle.is_null() {
        unsafe { dlclose(handle) };
    }
}

#[no_mangle]
pub(crate) extern "C" fn plenty_library_require_origin_v1(
    expected: *const c_void,
    actual: *const c_void,
) {
    if expected.is_null() || expected != actual {
        crate::fail("library object belongs to a different loaded instance");
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_library_contract_v1(
    handle: *mut c_void,
    discovery: *const u8,
    discovery_len: usize,
    expected: *const u8,
    expected_len: usize,
) -> u32 {
    // SAFETY: trusted wrappers supply byte slices and a live lookup lease.
    // The discovery function's native implementation is trusted to return live,
    // immutable bytes. Bounds reject malformed sizes; they cannot sandbox C.
    unsafe {
        if expected_len == 0 || expected_len > 4 * 1024 * 1024 {
            return LoadError::IncompatibleContract as u32;
        }
        let address = match symbol(handle, std::slice::from_raw_parts(discovery, discovery_len)) {
            Ok(address) => address,
            Err(error) => return error as u32,
        };
        let discover: unsafe extern "C" fn(*mut usize) -> *const u8 = std::mem::transmute(address);
        let mut actual_len = 0;
        let actual = discover(&mut actual_len);
        if actual.is_null() || actual_len != expected_len {
            return LoadError::IncompatibleContract as u32;
        }
        if std::slice::from_raw_parts(actual, actual_len)
            != std::slice::from_raw_parts(expected, expected_len)
        {
            return LoadError::IncompatibleContract as u32;
        }
        0
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[cfg(feature = "allocation-checks")]
    #[test]
    fn allocation_failure_preserves_output_and_lookup_uses_no_rust_allocation() {
        unsafe {
            let handle = open(b"libm.so.6").unwrap();
            let sentinel = ptr::dangling_mut::<c_void>();
            let mut output = sentinel;
            crate::accounting::fail_after(Some(0));
            let failed = plenty_library_open_v1(b"libm.so.6".as_ptr(), 9, &mut output);
            let found = symbol(handle, b"cos");
            let missing = symbol(handle, b"missing_plenty_test_symbol");
            plenty_library_close_v1(handle);
            crate::accounting::fail_after(None);
            assert_eq!(failed, LoadError::OutOfMemory as u32);
            assert_eq!(output, sentinel);
            assert!(found.is_ok());
            assert_eq!(missing, Err(LoadError::MissingSymbol));
        }
    }

    #[test]
    fn errors_leave_outputs_unchanged_and_code_survives_closed_lease() {
        unsafe {
            let sentinel = ptr::dangling_mut::<c_void>();
            let mut output = sentinel;
            for path in [b"".as_slice(), b"x\0y"] {
                assert_eq!(
                    plenty_library_open_v1(path.as_ptr(), path.len(), &mut output),
                    LoadError::InvalidPath as u32
                );
                assert_eq!(output, sentinel);
            }
            assert_eq!(
                open(b"/nonexistent/plenty-library-test.so"),
                Err(LoadError::OpenFailed)
            );
            let handle = open(b"libm.so.6").unwrap();
            assert_eq!(
                symbol(handle, b"missing_plenty_test_symbol"),
                Err(LoadError::MissingSymbol)
            );
            assert_eq!(
                symbol(handle, b"bad\0symbol"),
                Err(LoadError::InvalidSymbol)
            );
            assert_eq!(symbol(handle, &[b'a'; 512]), Err(LoadError::InvalidSymbol));
            let address = symbol(handle, b"cos").unwrap();
            plenty_library_close_v1(handle);
            let cos: unsafe extern "C" fn(f64) -> f64 = std::mem::transmute(address);
            assert_eq!(cos(0.0), 1.0);
            plenty_library_close_v1(ptr::null_mut());
        }
    }
}

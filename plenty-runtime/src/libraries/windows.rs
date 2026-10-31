//! Windows native loader ABI, cross-checked with Rust's Windows bindings and
//! Zig's kernel32 declarations. PIN retains a module until process termination,
//! matching the runtime's resident-code contract after lookup leases close.
use super::*;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

struct WidePath {
    pointer: *mut u16,
    len: usize,
}

impl WidePath {
    fn new(path: &[u8]) -> Result<Self, LoadError> {
        // Raw byte paths cannot represent arbitrary Windows UTF-16 filenames.
        // Accept UTF-8 explicitly rather than silently replacing invalid bytes.
        let path = std::str::from_utf8(path).map_err(|_| LoadError::InvalidPath)?;
        let len = path.encode_utf16().count();
        let pointer = memory::try_allocate::<u16, u16>(len)?;
        // SAFETY: the zeroed header plus len-element tail includes a final NUL.
        for (index, unit) in path.encode_utf16().enumerate() {
            unsafe { pointer.add(index).write(unit) };
        }
        Ok(Self { pointer, len })
    }
}

impl Drop for WidePath {
    fn drop(&mut self) {
        // SAFETY: the allocation retains its original element count and layout.
        unsafe { memory::free::<u16, u16>(self.pointer, self.len) };
    }
}

pub(super) fn open(path: &[u8]) -> Result<*mut c_void, LoadError> {
    let path = WidePath::new(path)?;
    // SAFETY: the path is NUL terminated. Zero flags select the ordinary native
    // loader search policy; the file argument is reserved and must be null.
    unsafe {
        let handle = LoadLibraryExW(path.pointer, ptr::null_mut(), 0);
        if handle.is_null() {
            return Err(LoadError::OpenFailed);
        }
        let mut pinned = ptr::null_mut();
        const PIN_FROM_ADDRESS: u32 = 0x1 | 0x4;
        // FROM_ADDRESS identifies this exact mapped instance (its image base),
        // avoiding a second name lookup that could pin a different DLL.
        if GetModuleHandleExW(PIN_FROM_ADDRESS, handle.cast(), &mut pinned) == 0 {
            FreeLibrary(handle);
            return Err(LoadError::OpenFailed);
        }
        Ok(handle)
    }
}

pub(super) unsafe fn symbol(
    handle: *mut c_void,
    name: *const u8,
) -> Result<*mut c_void, LoadError> {
    // SAFETY: caller holds a live lookup lease and supplies a terminated name.
    let address = unsafe { GetProcAddress(handle, name) };
    if address.is_null() {
        Err(LoadError::MissingSymbol)
    } else {
        Ok(address)
    }
}

pub(super) unsafe fn close(handle: *mut c_void) {
    // SAFETY: this consumes one LoadLibraryExW reference. PIN keeps its code and
    // static state resident even when this was the final ordinary reference.
    unsafe { FreeLibrary(handle) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_utf8_path_preserves_the_public_output() {
        let sentinel = ptr::dangling_mut::<c_void>();
        let mut output = sentinel;
        // SAFETY: the slice and output are live and disjoint.
        assert_eq!(
            unsafe { plenty_library_open_v1([0xff].as_ptr(), 1, &mut output) },
            LoadError::InvalidPath as u32
        );
        assert_eq!(output, sentinel);
    }

    #[test]
    fn paths_encode_unicode_as_utf16_and_reject_invalid_utf8() {
        assert!(matches!(
            WidePath::new(&[0xff]),
            Err(LoadError::InvalidPath)
        ));
        let path = WidePath::new("C:\\libraries\\音😀.dll".as_bytes()).unwrap();
        let expected: Vec<u16> = "C:\\libraries\\音😀.dll"
            .encode_utf16()
            .chain([0])
            .collect();
        // SAFETY: the buffer includes len UTF-16 units and a terminating NUL.
        assert_eq!(
            unsafe { std::slice::from_raw_parts(path.pointer, path.len + 1) },
            expected
        );
    }
}

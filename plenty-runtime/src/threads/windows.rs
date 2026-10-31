//! Use a native HANDLE in the existing opaque word, as Rust's Windows thread
//! implementation does (library/std/src/sys/thread/windows.rs). There is no
//! closure box: the compiler already pins the entry and its argument storage.
use super::*;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateThread(
        attributes: *const c_void,
        stack_size: usize,
        entry: unsafe extern "system" fn(*mut c_void) -> u32,
        argument: *mut c_void,
        flags: u32,
        id: *mut u32,
    ) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

unsafe extern "system" fn entry(storage: *mut c_void) -> u32 {
    // SAFETY: CreateThread supplies the pinned storage passed by the parent.
    unsafe { super::run(storage) };
    0
}

pub(super) unsafe fn start(storage: *mut usize) -> i32 {
    // SAFETY: the caller initialized the entry slot and pins storage until
    // join. Null attributes, id and zero flags select the ordinary defaults.
    unsafe {
        let handle = CreateThread(
            std::ptr::null(),
            0,
            entry,
            storage.cast(),
            0,
            std::ptr::null_mut(),
        );
        if handle.is_null() {
            return GetLastError() as i32;
        }
        storage.write(handle as usize);
    }
    0
}

pub(super) unsafe fn join(storage: *const usize) -> bool {
    // SAFETY: storage contains the handle from a successful start, with one
    // owner joining it. Only a completed wait permits reclaiming task storage.
    unsafe {
        let handle = storage.read() as *mut c_void;
        if WaitForSingleObject(handle, u32::MAX) != 0 {
            return false;
        }
        CloseHandle(handle) != 0
    }
}

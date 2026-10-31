use super::*;

// Linux uses an integer and Darwin a pointer for pthread_t. Generated code
// reserves one opaque word; check its size and alignment against the OS ABI.
const _: () = {
    assert!(size_of::<usize>() == size_of::<libc::pthread_t>());
    assert!(align_of::<usize>() == align_of::<libc::pthread_t>());
};

extern "C" fn entry(storage: *mut c_void) -> *mut c_void {
    // SAFETY: only pthread_create invokes this with the creator's live storage.
    unsafe { super::run(storage) }
}

pub(super) unsafe fn start(storage: *mut usize) -> i32 {
    // SAFETY: the caller provides pinned storage with an initialized entry.
    // Cast only the opaque id slot; libc supplies the platform's exact ABI.
    unsafe { libc::pthread_create(storage.cast(), std::ptr::null(), entry, storage.cast()) }
}

pub(super) unsafe fn join(storage: *const usize) -> bool {
    // SAFETY: exactly one parent joins a successfully created thread, and the
    // first storage word holds the pthread_t written by pthread_create.
    unsafe {
        libc::pthread_join(
            storage.cast::<libc::pthread_t>().read(),
            std::ptr::null_mut(),
        ) == 0
    }
}

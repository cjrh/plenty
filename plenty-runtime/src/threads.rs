//! Joinable native threads with caller-owned, pinned task storage.
//!
//! The compiler keeps arguments and result slots alive until join. There is no
//! Rust closure box or infallible allocation on the submission path. The OS owns
//! the thread stack; pthread_create reports resource exhaustion to the caller.
use std::ffi::c_void;

pub(crate) type Entry = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

// This is the x86_64 Linux GNU ABI, the compiler's only supported native target.
// Keep pthread_t opaque to generated code: it reserves one aligned machine word.
#[link(name = "pthread")]
extern "C" {
    fn pthread_create(
        thread: *mut usize,
        attributes: *const c_void,
        entry: Entry,
        argument: *mut c_void,
    ) -> i32;
    fn pthread_join(thread: usize, result: *mut *mut c_void) -> i32;
}

#[cfg(any(test, feature = "allocation-checks"))]
thread_local! {
    static STARTS_LEFT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(any(test, feature = "allocation-checks"))]
pub(crate) fn fail_after(starts: Option<usize>) {
    STARTS_LEFT.set(starts);
}

/// Every native thread enters here, so each one can report a stack overflow.
unsafe extern "C" fn run(storage: *mut c_void) -> *mut c_void {
    // SAFETY: `plenty_thread_start` stored the entry before creating this
    // thread, and the parent does not touch that word again.
    let entry = unsafe { storage.cast::<Entry>().add(1).read() };
    // SAFETY: the entry receives the storage its creator paired it with.
    crate::stack_overflow::guarded(|| unsafe { entry(storage) })
}

/// Start `entry(storage)` on a new thread. `storage` begins with two words
/// owned by the runtime: the thread id, then the entry for [`run`].
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_thread_start(storage: *mut usize, entry: Entry) -> i32 {
    #[cfg(any(test, feature = "allocation-checks"))]
    if STARTS_LEFT.with(|left| match left.get() {
        Some(0) => true,
        Some(n) => {
            left.set(Some(n - 1));
            false
        }
        None => false,
    }) {
        return 11; // EAGAIN on the supported target.
    }
    // SAFETY: generated code provides pinned, initialized task storage and a
    // matching C entry adapter. Only the parent accesses the thread-id word;
    // the worker owns its argument/result slots until pthread_join completes.
    unsafe {
        storage.cast::<Entry>().add(1).write(entry);
        pthread_create(storage, std::ptr::null(), run, storage.cast())
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_thread_join(storage: *const usize) {
    // SAFETY: exactly one parent joins this successfully started native thread.
    // The task cannot escape, self-join, be detached, or be canceled from Plenty.
    let status = unsafe { pthread_join(*storage, std::ptr::null_mut()) };
    if status != 0 {
        // Returning would permit the caller to reclaim storage still used by a
        // worker. This indicates a compiler/runtime contract violation.
        crate::fail("native thread join contract violated");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[repr(C)]
    struct Task {
        thread: usize,
        entry: usize,
        arrivals: *const AtomicUsize,
        result: usize,
    }

    unsafe extern "C" fn work(data: *mut c_void) -> *mut c_void {
        // SAFETY: the test pins each Task until its join completes. The parent
        // touches only the disjoint thread word before joining.
        unsafe {
            let task = data.cast::<Task>();
            let arrivals = &*(*task).arrivals;
            arrivals.fetch_add(1, Ordering::Release);
            while arrivals.load(Ordering::Acquire) < 2 {
                std::thread::yield_now();
            }
            (*task).result = 42;
        }
        std::ptr::null_mut()
    }

    #[test]
    fn native_workers_overlap_and_publish_results_at_join() {
        let arrivals = AtomicUsize::new(0);
        let mut a = Task {
            thread: 0,
            entry: 0,
            arrivals: &arrivals,
            result: 0,
        };
        let mut b = Task {
            thread: 0,
            entry: 0,
            arrivals: &arrivals,
            result: 0,
        };
        // SAFETY: both tasks remain at these addresses until both workers join.
        unsafe {
            assert_eq!(plenty_thread_start((&raw mut a).cast(), work), 0);
            assert_eq!(plenty_thread_start((&raw mut b).cast(), work), 0);
            plenty_thread_join((&raw const a).cast());
            plenty_thread_join((&raw const b).cast());
        }
        assert_eq!((a.result, b.result), (42, 42));
    }

    #[test]
    fn start_failure_never_runs_or_consumes_the_job() {
        let arrivals = AtomicUsize::new(0);
        let mut task = Task {
            thread: 0,
            entry: 0,
            arrivals: &arrivals,
            result: 99,
        };
        fail_after(Some(0));
        // SAFETY: failure injection prevents entry; storage is nevertheless valid.
        let status = unsafe { plenty_thread_start((&raw mut task).cast(), work) };
        fail_after(None);
        assert_eq!(status, 11);
        assert_eq!(task.result, 99);
        assert_eq!(arrivals.load(Ordering::Relaxed), 0);
    }
}

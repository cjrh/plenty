//! Windows delivers STATUS_STACK_OVERFLOW on the thread's guaranteed exception
//! stack. SetThreadStackGuarantee commits that reserve; unlike POSIX no alternate
//! signal stack is required. The guarantee only grows and is retained on return.
use crate::signal_context::{self, CONTINUE_SEARCH, ExceptionPointers, STACK_OVERFLOW};
use std::cell::Cell;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

static INSTALL: Once = Once::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);
thread_local! {
    // Constant TLS has no lazy allocation or destructor on the exception path.
    static GUARDED: Cell<bool> = const { Cell::new(false) };
}

#[cfg_attr(not(plenty_runtime_embedded), allow(dead_code))]
pub(crate) fn install() {
    INSTALL.call_once(|| {
        // SAFETY: the callback has the Windows exception ABI and lives forever.
        if unsafe { signal_context::AddVectoredExceptionHandler(1, on_fault) }.is_null() {
            signal_context::fatal(b"error: cannot install stack-overflow exception handler\n");
        }
        INSTALLED.store(true, Ordering::Release);
    });
}

pub(crate) fn guarded<R>(body: impl FnOnce() -> R) -> R {
    if !INSTALLED.load(Ordering::Acquire) {
        return body();
    }
    let mut bytes = 64 * 1024;
    // SAFETY: a writable DWORD requests the current thread's reserve. Windows
    // retains a larger preexisting guarantee, including a host's reservation.
    if unsafe { signal_context::SetThreadStackGuarantee(&mut bytes) } == 0 {
        signal_context::fatal(b"error: cannot reserve stack for overflow reporting\n");
    }
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            GUARDED.set(self.0);
        }
    }
    let _restore = Restore(GUARDED.replace(true));
    body()
}

unsafe extern "system" fn on_fault(pointers: *mut ExceptionPointers) -> i32 {
    // SAFETY: Windows passes a live exception record. Other thread/host faults
    // continue down the OS chain without changing their record or disposition.
    if unsafe { (*(*pointers).record).code } == STACK_OVERFLOW
        && GUARDED.try_with(Cell::get).unwrap_or(false)
    {
        signal_context::fatal(b"error: stack overflow\n");
    }
    CONTINUE_SEARCH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_guards_and_unwinding_restore_host_scope() {
        install();
        assert!(!GUARDED.get());
        guarded(|| {
            assert!(GUARDED.get());
            guarded(|| assert!(GUARDED.get()));
            assert!(GUARDED.get());
        });
        assert!(!GUARDED.get());
        let result = std::panic::catch_unwind(|| guarded(|| panic!("guard unwind probe")));
        assert!(result.is_err());
        assert!(!GUARDED.get());
    }
}

//! Stack-budget checks before entering a typed serialization call segment.
//!
//! The compiler supplies the complete segment budget, including frame setup,
//! temporary storage, and space for checking and reporting failure. These checks
//! do not install signal handlers and also work when called from a host library.
//! Unix bounds are cached per native thread; hosts must not change that thread's
//! stack mapping or reduce its stack resource limit while running Plenty code.
#[cfg(unix)]
use std::cell::Cell;

#[derive(Clone, Copy)]
struct Bounds {
    low: usize,
    high: usize,
}

#[cfg(unix)]
thread_local! {
    // No allocation or destructor. Cache failed discovery too: failure must
    // reject the call, rather than treating unknown bounds as unlimited space.
    static BOUNDS: Cell<Option<Option<Bounds>>> = const { Cell::new(None) };
}

/// Read the actual stack pointer. A local's address could sit above SP and
/// overstate the space available before the next callee's prologue.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline(always)]
fn stack_pointer() -> usize {
    let pointer: usize;
    // SAFETY: this only copies the stack-pointer register, preserves flags,
    // touches no memory, and does not modify the stack or any other register.
    unsafe {
        #[cfg(target_arch = "x86_64")]
        core::arch::asm!("mov {}, rsp", out(reg) pointer, options(nomem, nostack, preserves_flags));
        #[cfg(target_arch = "aarch64")]
        core::arch::asm!("mov {}, sp", out(reg) pointer, options(nomem, nostack, preserves_flags));
    }
    pointer
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn stack_pointer() -> usize {
    0 // Unsupported architectures fail closed.
}

fn fits(bounds: Bounds, pointer: usize, required: u64) -> bool {
    pointer >= bounds.low
        && pointer < bounds.high
        && usize::try_from(required)
            .ok()
            .is_some_and(|required| required <= pointer - bounds.low)
}

#[cfg(target_os = "linux")]
fn discover() -> Option<Bounds> {
    let mut attr = std::mem::MaybeUninit::<libc::pthread_attr_t>::uninit();
    // SAFETY: the query initializes this native attribute record on success.
    // glibc may allocate internally while discovering the main thread's stack
    // through procfs/RLIMIT_STACK; its reported failure rejects the call.
    if unsafe { libc::pthread_getattr_np(libc::pthread_self(), attr.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: successful initialization above; destroy on every path below.
    let mut attr = unsafe { attr.assume_init() };
    let mut base = std::ptr::null_mut();
    let mut size = 0;
    let mut guard = 0;
    // SAFETY: all output slots and the initialized attribute record are live.
    let (stack_result, guard_result, page) = unsafe {
        let stack = libc::pthread_attr_getstack(&attr, &mut base, &mut size);
        let guard_result = libc::pthread_attr_getguardsize(&attr, &mut guard);
        libc::pthread_attr_destroy(&mut attr);
        (stack, guard_result, libc::sysconf(libc::_SC_PAGESIZE))
    };
    if stack_result != 0 || guard_result != 0 || page <= 0 {
        return None;
    }
    let base = base as usize;
    let high = base.checked_add(size)?;
    // pthread implementations differ in whether guard bytes are included in
    // the reported stack extent. Adding them is conservative in either case.
    // Keep one more page untouched, including main threads with guard size 0.
    let low = base.checked_add(guard)?.checked_add(page as usize)?;
    (base != 0 && low < high).then_some(Bounds { low, high })
}

#[cfg(target_os = "macos")]
fn discover() -> Option<Bounds> {
    // SAFETY: these queries borrow only the current, live pthread handle.
    let (high, size, page) = unsafe {
        let thread = libc::pthread_self();
        (
            libc::pthread_get_stackaddr_np(thread) as usize,
            libc::pthread_get_stacksize_np(thread),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    if page <= 0 {
        return None;
    }
    let low = high.checked_sub(size)?.checked_add(page as usize)?;
    (low != 0 && low < high).then_some(Bounds { low, high })
}

#[cfg(windows)]
fn discover() -> Option<Bounds> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadStackLimits(low: *mut usize, high: *mut usize);
    }
    let mut low = 0;
    let mut high = 0;
    // SAFETY: Windows writes two live SIZE_T output slots for this thread.
    unsafe { GetCurrentThreadStackLimits(&mut low, &mut high) };
    // The compiler's recovery budget covers the guard and exception reserve.
    (low != 0 && low < high).then_some(Bounds { low, high })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn discover() -> Option<Bounds> {
    None
}

fn bounds() -> Option<Bounds> {
    #[cfg(unix)]
    {
        BOUNDS.with(|cached| match cached.get() {
            Some(bounds) => bounds,
            None => {
                let bounds = discover();
                cached.set(Some(bounds));
                bounds
            }
        })
    }
    #[cfg(not(unix))]
    {
        // Windows' committed lower limit can change as the stack grows. Query
        // each time rather than caching an overly optimistic old boundary.
        discover()
    }
}

/// Return whether the current native thread has `required` bytes available.
/// The compiler calls this before the protected callee allocates its frame.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn plenty_serial_stack_ready(required: u64) -> u8 {
    let Some(bounds) = bounds() else {
        return 0;
    };
    u8::from(fits(bounds, stack_pointer(), required))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_and_foreign_stack_pointers_fail_closed() {
        let bounds = Bounds {
            low: 100,
            high: 300,
        };
        assert!(fits(bounds, 200, 100));
        assert!(!fits(bounds, 200, 101));
        assert!(!fits(bounds, 99, 0));
        assert!(!fits(bounds, 300, 0));
        assert!(!fits(bounds, 200, u64::MAX));
    }

    #[test]
    fn native_thread_budget_is_bounded_without_signal_handler() {
        assert_eq!(plenty_serial_stack_ready(0), 1);
        assert_eq!(plenty_serial_stack_ready(1024), 1);
        assert_eq!(plenty_serial_stack_ready(u64::MAX), 0);
        let bounds = bounds().expect("supported native stack bounds");
        let pointer = stack_pointer();
        assert!(fits(bounds, pointer, (pointer - bounds.low) as u64));
        assert!(!fits(bounds, pointer, (pointer - bounds.low) as u64 + 1));
    }

    #[test]
    fn small_workers_initialize_independent_bounds() {
        for size in [64 * 1024, 128 * 1024] {
            std::thread::Builder::new()
                .stack_size(size)
                .spawn(move || {
                    assert_eq!(plenty_serial_stack_ready(1024), 1);
                    assert_eq!(plenty_serial_stack_ready(u64::MAX), 0);
                    let bounds = bounds().expect("worker native bounds");
                    // An independent worker must not inherit the main thread's
                    // TLS limits, even after the main thread queried its stack.
                    assert!(bounds.high - bounds.low <= size);
                    assert_eq!(plenty_serial_stack_ready(size as u64), 0);
                })
                .expect("create small-stack worker")
                .join()
                .expect("small-stack check completes without overflow");
        }
    }
}

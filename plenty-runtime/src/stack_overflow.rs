//! Reports native stack exhaustion instead of dying on a bare `SIGSEGV`.
//!
//! A handler needs a stack of its own, since the faulting thread has none left.
//! Each thread running Plenty code reserves one in the frame that calls into
//! generated code, at the top of its own stack. Exhaustion faults at the other
//! end, so the reservation is still intact, and entering a thread costs one
//! system call with no allocation or mapping.
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::signal_context;
use libc::{
    SA_ONSTACK, SA_SIGINFO, SIGSEGV, SS_ONSTACK, abort, raise, sigaction, sigaltstack, write,
};
use std::sync::atomic::AtomicUsize;

// Includes the fixed ARM context and extended register records. Refuse to
// install if the kernel advertises a larger minimum signal frame.
#[cfg(target_arch = "aarch64")]
const RESERVED: usize = 64 * 1024;
#[cfg(target_arch = "x86_64")]
const RESERVED: usize = 16 * 1024;
#[cfg(target_arch = "aarch64")]
const HANDLER_MARGIN: usize = 8 * 1024;
#[cfg(target_arch = "x86_64")]
const HANDLER_MARGIN: usize = 2 * 1024;
static PAGE_BYTES: AtomicUsize = AtomicUsize::new(4096);

const MESSAGE: &[u8] = b"error: stack overflow\n";

type SignalStack = libc::stack_t;
type Info = libc::siginfo_t;

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Claim `SIGSEGV` for the process. Only the executable entry point calls this:
/// a Plenty library leaves signal dispositions to its host.
#[cfg_attr(not(plenty_runtime_embedded), allow(dead_code))]
pub(crate) fn install() {
    // SAFETY: these queries take no pointers and run outside signal handlers.
    let (page, minimum) = unsafe {
        (
            libc::sysconf(libc::_SC_PAGESIZE),
            libc::getauxval(libc::AT_MINSIGSTKSZ),
        )
    };
    if page <= 0 || minimum as usize > RESERVED - HANDLER_MARGIN {
        return;
    }
    PAGE_BYTES.store(page as usize, Ordering::Relaxed);
    let action = signal_context::action(on_fault as *const () as usize, SA_SIGINFO | SA_ONSTACK);
    let mut previous = MaybeUninit::<libc::sigaction>::uninit();
    // SAFETY: both records have the ABI layout and outlive the calls.
    unsafe {
        // A handler present before `main`, such as a sanitizer's, keeps the signal.
        if sigaction(SIGSEGV, null(), previous.as_mut_ptr()) == 0
            && previous.assume_init().sa_sigaction == libc::SIG_DFL
            && sigaction(SIGSEGV, &action, null_mut()) == 0
        {
            INSTALLED.store(true, Ordering::Relaxed);
        }
    }
}

/// Run `body` with an alternate signal stack reserved in this frame. Every
/// frame `body` pushes lies below the reservation.
pub(crate) fn guarded<R>(body: impl FnOnce() -> R) -> R {
    if !INSTALLED.load(Ordering::Relaxed) {
        return body();
    }
    let mut reserved = MaybeUninit::<[u8; RESERVED]>::uninit();
    let stack = SignalStack {
        ss_sp: reserved.as_mut_ptr().cast(),
        ss_flags: 0,
        ss_size: RESERVED,
    };
    // SAFETY: the reservation outlives its registration, which ends below.
    // A refusal leaves the thread without a report, as before.
    let mut previous = MaybeUninit::<SignalStack>::uninit();
    let registered = unsafe { sigaltstack(&stack, previous.as_mut_ptr()) } == 0;
    let result = body();
    if registered {
        // SAFETY: successful registration initialized the previous stack;
        // restoring it preserves a host's alternate stack and nested guards.
        unsafe { sigaltstack(previous.as_ptr(), null_mut()) };
    }
    result
}

/// Whether a fault at `address` is the thread running out of stack: a data
/// access between just below its stack pointer and the top of its stack. The
/// range below `stack_pointer` is where a call, a frame, or a stack probe
/// touches new stack. The range above it is the live stack, which faults only
/// when the stack pointer itself has passed the end of the stack.
fn exhausted(address: usize, stack_pointer: usize, top: usize, instruction: bool) -> bool {
    !instruction
        && stack_pointer < top
        && address < top
        && address >= stack_pointer.saturating_sub(PAGE_BYTES.load(Ordering::Relaxed))
}

unsafe extern "C" fn on_fault(_signal: i32, info: *const Info, context: *const c_void) {
    // Only async-signal-safe calls are made here: no allocation and no locks.
    // SAFETY: the kernel passes live records with the declared layouts.
    unsafe {
        let context = &*context.cast::<libc::ucontext_t>();
        // The kernel reports its own faults with a positive code. A signal
        // sent by a process carries no fault address.
        let fault = (*info).si_code > 0;
        let mut stack = MaybeUninit::<SignalStack>::uninit();
        // Running on an alternate stack shows this thread reserved one in
        // `guarded`, so its base is the top of the stack that code runs on.
        if fault && sigaltstack(null(), stack.as_mut_ptr()) == 0 {
            let stack = stack.assume_init();
            if stack.ss_flags & SS_ONSTACK != 0
                && exhausted(
                    (*info).si_addr() as usize,
                    signal_context::sp(context),
                    stack.ss_sp as usize,
                    signal_context::instruction_fault(context, (*info).si_addr() as usize),
                )
            {
                write(2, MESSAGE.as_ptr().cast(), MESSAGE.len());
                abort();
            }
        }
        // Any other fault reruns its instruction under the default action.
        let default = signal_context::action(libc::SIG_DFL, 0);
        sigaction(SIGSEGV, &default, null_mut());
        if !fault {
            // Nothing reruns, so deliver the signal again once this returns.
            raise(SIGSEGV);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: usize = 0x7000_0000;

    #[test]
    fn stack_growth_below_the_stack_pointer_is_exhaustion() {
        let sp = TOP - 0x8000;
        assert!(exhausted(sp - 8, sp, TOP, false));
        assert!(exhausted(
            sp - PAGE_BYTES.load(Ordering::Relaxed),
            sp,
            TOP,
            false
        ));
        assert!(exhausted(sp, sp, TOP, false));
        assert!(exhausted(TOP - 1, sp, TOP, false));
    }

    #[test]
    fn other_faults_are_not_exhaustion() {
        let sp = TOP - 0x8000;
        assert!(!exhausted(0, sp, TOP, false));
        assert!(!exhausted(
            sp - PAGE_BYTES.load(Ordering::Relaxed) - 1,
            sp,
            TOP,
            false
        ));
        assert!(!exhausted(TOP, sp, TOP, false));
        assert!(!exhausted(sp - 8, sp, TOP, true));
        // A stack pointer above the reservation belongs to another stack.
        assert!(!exhausted(TOP - 8, TOP + 0x1000, TOP, false));
    }

    #[test]
    fn an_uninstalled_guard_only_runs_its_body() {
        assert_eq!(guarded(|| 42), 42);
    }
}

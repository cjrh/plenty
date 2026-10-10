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

// The constants and layouts below are the x86_64 Linux GNU ABI, the compiler's
// only supported native target.
const SIGSEGV: i32 = 11;
const SA_SIGINFO: i32 = 4;
const SA_ONSTACK: i32 = 0x0800_0000;
const SS_ONSTACK: i32 = 1;
const SS_DISABLE: i32 = 2;
const REG_RSP: usize = 15;
const REG_ERR: usize = 19;
/// Page-fault error bit for an instruction fetch.
const INSTRUCTION_FETCH: u64 = 1 << 4;

/// Holds the kernel's signal frame, including the widest non-AMX register
/// state (about 4 KiB with AVX-512), plus the handler's own small frames.
const RESERVED: usize = 16 * 1024;

/// Code may write this far below its stack pointer before moving it: the
/// System V red zone is 128 bytes and a call pushes 8. A page leaves margin.
const BELOW_STACK_POINTER: usize = 4096;

const MESSAGE: &[u8] = b"error: stack overflow\n";

#[repr(C)]
struct Action {
    handler: usize,
    mask: [u64; 16],
    flags: i32,
    restorer: usize,
}

#[repr(C)]
struct SignalStack {
    base: *mut c_void,
    flags: i32,
    size: usize,
}

#[repr(C)]
struct Info {
    signal: i32,
    errno: i32,
    code: i32,
    address: usize,
}

#[repr(C)]
struct Context {
    flags: u64,
    link: usize,
    stack: SignalStack,
    registers: [u64; 23],
}

const _: () = {
    assert!(size_of::<Action>() == 152);
    assert!(size_of::<SignalStack>() == 24);
    assert!(std::mem::offset_of!(Info, address) == 16);
    assert!(std::mem::offset_of!(Context, registers) == 40);
};

extern "C" {
    fn sigaction(signal: i32, action: *const Action, previous: *mut Action) -> i32;
    fn sigaltstack(stack: *const SignalStack, previous: *mut SignalStack) -> i32;
    fn write(descriptor: i32, bytes: *const c_void, count: usize) -> isize;
    fn raise(signal: i32) -> i32;
    fn abort() -> !;
}

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Claim `SIGSEGV` for the process. Only the executable entry point calls this:
/// a Plenty library leaves signal dispositions to its host.
#[cfg_attr(not(plenty_runtime_embedded), allow(dead_code))]
pub(crate) fn install() {
    let action = Action {
        handler: on_fault as *const () as usize,
        mask: [0; 16],
        flags: SA_SIGINFO | SA_ONSTACK,
        restorer: 0,
    };
    let mut previous = MaybeUninit::<Action>::uninit();
    // SAFETY: both records have the ABI layout and outlive the calls.
    unsafe {
        // A handler present before `main`, such as a sanitizer's, keeps the signal.
        if sigaction(SIGSEGV, null(), previous.as_mut_ptr()) == 0
            && previous.assume_init().handler == 0
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
        base: reserved.as_mut_ptr().cast(),
        flags: 0,
        size: RESERVED,
    };
    // SAFETY: the reservation outlives its registration, which ends below.
    // A refusal leaves the thread without a report, as before.
    let registered = unsafe { sigaltstack(&stack, null_mut()) } == 0;
    let result = body();
    if registered {
        let disabled = SignalStack {
            base: null_mut(),
            flags: SS_DISABLE,
            size: 0,
        };
        // SAFETY: this thread is not running on the stack it unregisters.
        unsafe { sigaltstack(&disabled, null_mut()) };
    }
    result
}

/// Whether a fault at `address` is the thread running out of stack: a data
/// access between just below its stack pointer and the top of its stack. The
/// range below `stack_pointer` is where a call, a frame, or a stack probe
/// touches new stack. The range above it is the live stack, which faults only
/// when the stack pointer itself has passed the end of the stack.
fn exhausted(address: usize, stack_pointer: usize, top: usize, error: u64) -> bool {
    error & INSTRUCTION_FETCH == 0
        && stack_pointer < top
        && address < top
        && address >= stack_pointer.saturating_sub(BELOW_STACK_POINTER)
}

unsafe extern "C" fn on_fault(_signal: i32, info: *const Info, context: *const c_void) {
    // Only async-signal-safe calls are made here: no allocation and no locks.
    // SAFETY: the kernel passes live records with the declared layouts.
    unsafe {
        let registers = &(*context.cast::<Context>()).registers;
        // The kernel reports its own faults with a positive code. A signal
        // sent by a process carries no fault address.
        let fault = (*info).code > 0;
        let mut stack = MaybeUninit::<SignalStack>::uninit();
        // Running on an alternate stack shows this thread reserved one in
        // `guarded`, so its base is the top of the stack that code runs on.
        if fault && sigaltstack(null(), stack.as_mut_ptr()) == 0 {
            let stack = stack.assume_init();
            if stack.flags & SS_ONSTACK != 0
                && exhausted(
                    (*info).address,
                    registers[REG_RSP] as usize,
                    stack.base as usize,
                    registers[REG_ERR],
                )
            {
                write(2, MESSAGE.as_ptr().cast(), MESSAGE.len());
                abort();
            }
        }
        // Any other fault reruns its instruction under the default action.
        let default = Action {
            handler: 0,
            mask: [0; 16],
            flags: 0,
            restorer: 0,
        };
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
        assert!(exhausted(sp - 8, sp, TOP, 0));
        assert!(exhausted(sp - BELOW_STACK_POINTER, sp, TOP, 0));
        assert!(exhausted(sp, sp, TOP, 0));
        assert!(exhausted(TOP - 1, sp, TOP, 0));
    }

    #[test]
    fn other_faults_are_not_exhaustion() {
        let sp = TOP - 0x8000;
        assert!(!exhausted(0, sp, TOP, 0));
        assert!(!exhausted(sp - BELOW_STACK_POINTER - 1, sp, TOP, 0));
        assert!(!exhausted(TOP, sp, TOP, 0));
        assert!(!exhausted(sp - 8, sp, TOP, INSTRUCTION_FETCH));
        // A stack pointer above the reservation belongs to another stack.
        assert!(!exhausted(TOP - 8, TOP + 0x1000, TOP, 0));
    }

    #[test]
    fn an_uninstalled_guard_only_runs_its_body() {
        assert_eq!(guarded(|| 42), 42);
    }
}

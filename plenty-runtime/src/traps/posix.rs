//! POSIX checked-instruction delivery and saved signal dispositions.
use super::*;
use libc::{SA_ONSTACK, SA_SIGINFO, SIGILL, raise, sigaction};
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
type Action = libc::sigaction;
type Info = libc::siginfo_t;

/// The disposition `SIGILL` had before registration. Immutable after TABLE
/// publication; unrelated signals call it without replacing our disposition.
static mut PREVIOUS: MaybeUninit<Action> = MaybeUninit::uninit();
// SA_RESETHAND consumes only the saved action, never the checked-trap handler.
static PREVIOUS_RESET: AtomicBool = AtomicBool::new(false);

/// Called once by `plenty_main`, before any check can fail.
///
/// # Safety
/// `table` must point to the compiler's table, which lives as long as the
/// process.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_traps_register(table: *const c_void) {
    let mut action = signal_context::action(on_illegal as *const () as usize, SA_SIGINFO);
    // SAFETY: registration runs once, on the main thread, before the handler
    // can read `PREVIOUS`; both records have the ABI layout.
    unsafe {
        let previous = &raw mut PREVIOUS;
        if sigaction(SIGILL, std::ptr::null(), (*previous).as_mut_ptr()) != 0 {
            return;
        }
        // Preserve interrupted-syscall behavior for the action we forward to.
        let previous = (*previous).assume_init_ref();
        action.sa_mask = previous.sa_mask;
        action.sa_flags |= previous.sa_flags & libc::SA_RESTART;
        // Custom actions retain their stack choice. Otherwise checked traps
        // keep their alternate-stack margin. Never switch stacks around a
        // callback: nested ONSTACK delivery could overwrite suspended frames.
        if previous.sa_flags & SA_ONSTACK != 0
            || matches!(previous.sa_sigaction, libc::SIG_DFL | libc::SIG_IGN)
        {
            action.sa_flags |= SA_ONSTACK;
        }
        PREVIOUS_RESET.store(false, Ordering::Relaxed);
        TABLE.store(table.cast_mut().cast(), Ordering::Release);
        // A refusal leaves checks to the previous disposition: the process
        // still dies on the illegal instruction, without a message.
        sigaction(SIGILL, &action, null_mut());
    }
}

/// Dispatch a saved action on this thread without temporarily changing the
/// process-wide disposition: another thread can trap while this callback runs.
unsafe fn forward(
    previous: &Action,
    signal: i32,
    info: *const Info,
    context: &mut libc::ucontext_t,
    fault: bool,
) {
    let mut handler = previous.sa_sigaction;
    if !matches!(handler, libc::SIG_DFL | libc::SIG_IGN)
        && previous.sa_flags & libc::SA_RESETHAND != 0
        && PREVIOUS_RESET.swap(true, Ordering::AcqRel)
    {
        handler = libc::SIG_DFL;
    }
    if handler == libc::SIG_IGN && !fault {
        return;
    }
    // Ignoring a synchronous instruction fault cannot make forward progress.
    if handler == libc::SIG_DFL || handler == libc::SIG_IGN {
        let default = signal_context::action(libc::SIG_DFL, 0);
        // SAFETY: sigaction and raise are async-signal-safe. A hardware fault
        // repeats under SIG_DFL; a raised signal must be sent again explicitly.
        unsafe {
            sigaction(signal, &default, null_mut());
            if !fault {
                raise(signal);
            }
        }
        return;
    }
    // The kernel applied our action's mask. Reconstruct the saved action's
    // mask from the pre-signal context before calling it, including NODEFER.
    // Rebuild the union using the native signal-number range.
    unsafe {
        #[cfg(target_os = "linux")]
        let errno = libc::__errno_location();
        #[cfg(target_os = "macos")]
        let errno = libc::__error();
        let saved_errno = *errno;
        let mut mask = context.uc_sigmask;
        #[cfg(target_os = "linux")]
        let last_signal = 64;
        #[cfg(target_os = "macos")]
        let last_signal = 31;
        for signal in 1..=last_signal {
            if libc::sigismember(&previous.sa_mask, signal) == 1 {
                libc::sigaddset(&mut mask, signal);
            }
        }
        if previous.sa_flags & libc::SA_NODEFER == 0 {
            libc::sigaddset(&mut mask, signal);
        }
        let mut restore = MaybeUninit::<libc::sigset_t>::uninit();
        let changed = libc::sigprocmask(libc::SIG_SETMASK, &mask, restore.as_mut_ptr()) == 0;
        // SAFETY: sigaction supplies a function pointer with the signature
        // selected by SA_SIGINFO. Context edits remain visible to sigreturn.
        *errno = saved_errno;
        if previous.sa_flags & SA_SIGINFO != 0 {
            let action: unsafe extern "C" fn(i32, *mut Info, *mut c_void) =
                std::mem::transmute(handler);
            action(
                signal,
                info.cast_mut(),
                (context as *mut libc::ucontext_t).cast(),
            );
        } else {
            let action: unsafe extern "C" fn(i32) = std::mem::transmute(handler);
            action(signal);
        }
        let handler_errno = *errno;
        if changed {
            libc::sigprocmask(libc::SIG_SETMASK, restore.as_ptr(), null_mut());
        }
        *errno = handler_errno;
    }
}

unsafe extern "C" fn on_illegal(_signal: i32, info: *const Info, context: *mut c_void) {
    // Only async-signal-safe calls are made here: no allocation and no locks.
    // SAFETY: the kernel passes live records with the declared layouts, and
    // `TABLE` is the compiler's table once set.
    unsafe {
        let context = &mut *context.cast::<libc::ucontext_t>();
        // Only hardware faults identify an instruction to retry; user-sent
        // signals have OS-specific codes and rerun nothing.
        let fault = signal_context::hardware_fault((*info).si_code);
        let table = TABLE.load(Ordering::Acquire);
        if fault
            && !table.is_null()
            && let Some((helper, site, option)) = lookup(table, signal_context::pc(context))
        {
            // Resume in the native C helper, respecting the architecture's
            // stack and argument-register convention. The helper never returns.
            signal_context::enter_helper(context, helper, site as usize, option);
            return;
        }
        let previous = &raw const PREVIOUS;
        forward((*previous).assume_init_ref(), SIGILL, info, context, fault);
    }
}

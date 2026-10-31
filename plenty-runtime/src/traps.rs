//! Reports failed checks that end in a native illegal instruction.
//!
//! An executable's failed check jumps to `ud2` (x86-64) or `udf` (AArch64).
//! The compiler lists every such address in a table that `plenty_main` registers first
//! (`src/codegen/traps.rs`). The `SIGILL` handler finds the faulting address
//! there and resumes the thread in the helper a call would have reached, with
//! the check's site as its argument, so the report runs in the thread's own
//! context exactly as after a call: same message, caller note and exit status.
//! Any other `SIGILL` reruns under the disposition found at registration.
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::Site;

use crate::signal_context;
use libc::{SA_ONSTACK, SA_SIGINFO, SIGILL, raise, sigaction};

/// The header of the compiler's table, followed by `count` fault-instruction addresses
/// and then `count` `u32`s: each check's site record offset, with its kind in
/// the low four bits. `sites` and the addresses are offsets from the field
/// holding them, which the linker resolves; `sites` is zero for none.
/// Kind 6 with `NO_SITE` marks an AArch64 loop-probe store for SIGSEGV.
#[repr(C)]
struct Table {
    count: u32,
    sites: i32,
}
const _: () = assert!(size_of::<Table>() == 8);

/// The address `field` holds as an offset from itself.
///
/// # Safety
/// `field` must be readable.
unsafe fn resolve(field: *const i32) -> usize {
    // SAFETY: as the caller guarantees.
    (field as usize).wrapping_add_signed(unsafe { *field } as isize)
}

const KIND_MASK: u32 = 15;
const NO_SITE: u32 = !KIND_MASK;

type Action = libc::sigaction;
type Info = libc::siginfo_t;

static TABLE: AtomicPtr<Table> = AtomicPtr::new(null_mut());
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

/// The helper and its arguments for the check whose illegal instruction is at `address`.
///
/// # Safety
/// `table` must point to a header followed by its arrays.
unsafe fn lookup(table: *const Table, address: usize) -> Option<(usize, *const Site, u64)> {
    // SAFETY: as the caller guarantees.
    let (sites, index, entries) = unsafe {
        let count = (*table).count as usize;
        let addresses = table.add(1).cast::<i32>();
        let sites = &raw const (*table).sites;
        (
            (*sites != 0).then(|| resolve(sites)),
            (0..count).position(|i| resolve(addresses.add(i)) == address)?,
            addresses.add(count).cast::<u32>(),
        )
    };
    // SAFETY: `index` is below the table's count.
    let entry = unsafe { *entries.add(index) };
    let site = match (entry & !KIND_MASK, sites) {
        (NO_SITE, _) | (_, None) => std::ptr::null(),
        (site, Some(sites)) => sites.wrapping_add(site as usize) as *const Site,
    };
    let kind = entry & KIND_MASK;
    let helper = match kind {
        0 => crate::io::plenty_trap_overflow as *const () as usize,
        1 => crate::io::plenty_trap_div_zero as *const () as usize,
        2 => crate::io::plenty_trap_shift as *const () as usize,
        3 => crate::io::plenty_trap_cast_range as *const () as usize,
        4 | 5 => crate::io::plenty_trap_unwrap as *const () as usize,
        _ => return None,
    };
    Some((helper, site, u64::from(kind == 5)))
}

/// Whether the compiler registered this exact PC as an AArch64 loop-probe
/// store. Reading the immutable table is allocation-free and signal-safe.
#[cfg(target_arch = "aarch64")]
pub(crate) fn stack_probe(address: usize) -> bool {
    let table = TABLE.load(Ordering::Acquire);
    if table.is_null() {
        return false;
    }
    // SAFETY: registration publishes a process-lifetime compiler table.
    unsafe {
        let count = (*table).count as usize;
        let addresses = table.add(1).cast::<i32>();
        let entries = addresses.add(count).cast::<u32>();
        (0..count).any(|index| {
            resolve(addresses.add(index)) == address && *entries.add(index) == (NO_SITE | 6)
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct Fixture {
        table: Table,
        addresses: [i32; 3],
        entries: [u32; 3],
        sites: [u8; 64],
        code: [u8; 3],
    }

    /// `target` as an offset from `field`.
    fn offset(field: *const i32, target: usize) -> i32 {
        i32::try_from(target.wrapping_sub(field as usize) as isize).unwrap()
    }

    #[test]
    fn lookup_maps_each_kind_and_site() {
        // Keep every relative target in the same allocation. Heap-to-stack
        // distances depend on address-space placement and can exceed i32.
        let mut fixture = Box::new(Fixture {
            table: Table { count: 3, sites: 0 },
            addresses: [0; 3],
            entries: [16, NO_SITE | 5, 4],
            sites: [0; 64],
            code: [0; 3],
        });
        let sites = fixture.sites.as_ptr();
        let code = fixture.code.as_ptr() as usize;
        let at = |i: usize| code + i;
        fixture.table.sites = offset(&raw const fixture.table.sites, sites as usize);
        for i in 0..3 {
            fixture.addresses[i] = offset(&raw const fixture.addresses[i], at(i));
        }
        let table = (&raw const *fixture).cast::<Table>();
        // SAFETY: the fixture is a header followed by its arrays.
        let lookup = |address| unsafe { lookup(table, address) };
        let (helper, site, option) = lookup(at(0)).unwrap();
        assert_eq!(
            helper,
            crate::io::plenty_trap_overflow as *const () as usize
        );
        assert_eq!(site.cast::<u8>(), sites.wrapping_add(16));
        assert_eq!(option, 0);
        let (helper, site, option) = lookup(at(1)).unwrap();
        assert_eq!(helper, crate::io::plenty_trap_unwrap as *const () as usize);
        assert!(site.is_null());
        assert_eq!(option, 1);
        assert_eq!(lookup(at(2)).unwrap().2, 0);
        assert!(lookup(at(2) + 1).is_none());
    }
}

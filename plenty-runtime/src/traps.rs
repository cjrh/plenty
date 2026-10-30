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
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::Site;

use crate::signal_context;
use libc::{SA_ONSTACK, SA_SIGINFO, SIGILL, raise, sigaction};

/// The header of the compiler's table, followed by `count` illegal-instruction addresses
/// and then `count` `u32`s: each check's site record offset, with its kind in
/// the low four bits. `sites` and the addresses are offsets from the field
/// holding them, which the linker resolves; `sites` is zero for none.
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
/// The disposition `SIGILL` had before registration, restored for any
/// `SIGILL` that is not a check. Written before `TABLE` is published.
static mut PREVIOUS: MaybeUninit<Action> = MaybeUninit::uninit();

/// Called once by `plenty_main`, before any check can fail.
///
/// # Safety
/// `table` must point to the compiler's table, which lives as long as the
/// process.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_traps_register(table: *const c_void) {
    let action = signal_context::action(on_illegal as *const () as usize, SA_SIGINFO | SA_ONSTACK);
    // SAFETY: registration runs once, on the main thread, before the handler
    // can read `PREVIOUS`; both records have the ABI layout.
    unsafe {
        let previous = &raw mut PREVIOUS;
        if sigaction(SIGILL, std::ptr::null(), (*previous).as_mut_ptr()) != 0 {
            return;
        }
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

unsafe extern "C" fn on_illegal(_signal: i32, info: *const Info, context: *mut c_void) {
    // Only async-signal-safe calls are made here: no allocation and no locks.
    // SAFETY: the kernel passes live records with the declared layouts, and
    // `TABLE` is the compiler's table once set.
    unsafe {
        let context = &mut *context.cast::<libc::ucontext_t>();
        // The kernel reports its own faults with a positive code; a signal
        // sent by a process reruns nothing.
        let fault = (*info).si_code > 0;
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
        sigaction(SIGILL, (*previous).as_ptr(), null_mut());
        if !fault {
            raise(SIGILL);
        }
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

//! Reports failed checks that end in `ud2` instead of a helper call.
//!
//! An executable's failed check jumps to a `ud2`, and the compiler lists
//! every such address in a table that `plenty_main` registers first
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

// The constants and layouts below are the x86_64 Linux GNU ABI, the compiler's
// only supported native target.
const SIGILL: i32 = 4;
const SA_SIGINFO: i32 = 4;
const SA_ONSTACK: i32 = 0x0800_0000;
const REG_RDI: usize = 8;
const REG_RSI: usize = 9;
const REG_RSP: usize = 15;
const REG_RIP: usize = 16;

/// The header of the compiler's table, followed by `count` `ud2` addresses
/// and then `count` `u32`s: each check's site record offset, with its kind in
/// the low four bits. `sites` and the addresses are offsets from the field
/// holding them, which the linker resolves; `sites` is zero for none.
#[repr(C)]
struct Table {
    count: u32,
    sites: i32,
}

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

#[repr(C)]
struct Action {
    handler: usize,
    mask: [u64; 16],
    flags: i32,
    restorer: usize,
}

#[repr(C)]
struct Info {
    signal: i32,
    errno: i32,
    code: i32,
}

#[repr(C)]
struct Context {
    flags: u64,
    link: usize,
    stack: [u64; 3],
    registers: [u64; 23],
}

const _: () = {
    assert!(size_of::<Action>() == 152);
    assert!(size_of::<Table>() == 8);
    assert!(std::mem::offset_of!(Context, registers) == 40);
};

unsafe extern "C" {
    fn sigaction(signal: i32, action: *const Action, previous: *mut Action) -> i32;
    fn raise(signal: i32) -> i32;
}

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
    let action = Action {
        handler: on_illegal as *const () as usize,
        mask: [0; 16],
        flags: SA_SIGINFO | SA_ONSTACK,
        restorer: 0,
    };
    // SAFETY: registration runs once, on the main thread, before the handler
    // can read `PREVIOUS`; both records have the ABI layout.
    unsafe {
        let previous = &raw mut PREVIOUS;
        if sigaction(SIGILL, std::ptr::null(), (*previous).as_mut_ptr()) != 0 {
            return;
        }
        TABLE.store(table.cast_mut().cast(), Ordering::Release);
        // A refusal leaves checks to the previous disposition: the process
        // still dies on the `ud2`, without a message.
        sigaction(SIGILL, &action, null_mut());
    }
}

/// The helper and its arguments for the check whose `ud2` is at `address`.
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
        let registers = &mut (*context.cast::<Context>()).registers;
        // The kernel reports its own faults with a positive code; a signal
        // sent by a process reruns nothing.
        let fault = (*info).code > 0;
        let table = TABLE.load(Ordering::Acquire);
        if fault
            && !table.is_null()
            && let Some((helper, site, option)) = lookup(table, registers[REG_RIP] as usize)
        {
            // Resume as if the check had called `helper`: it never returns,
            // so the return address slot stays unwritten, and the stack
            // pointer has the alignment a call leaves.
            registers[REG_RDI] = site as u64;
            registers[REG_RSI] = option;
            registers[REG_RSP] = (registers[REG_RSP] & !15) - 8;
            registers[REG_RIP] = helper as u64;
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

//! Windows vectored exception dispatch preserves the remaining host chain by
//! returning CONTINUE_SEARCH for every instruction outside our immutable table.
use super::*;
use signal_context::{CONTINUE_EXECUTION, CONTINUE_SEARCH, ExceptionPointers, ILLEGAL_INSTRUCTION};
use std::sync::Once;

static INSTALL: Once = Once::new();

/// # Safety
/// `table` must be the process-lifetime compiler-generated trap table.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn plenty_traps_register(table: *const c_void) {
    TABLE.store(table.cast_mut().cast(), Ordering::Release);
    INSTALL.call_once(|| {
        // SAFETY: the callback has the Windows exception-handler ABI and is
        // process-lifetime; no removal can race with exception delivery.
        if unsafe { signal_context::AddVectoredExceptionHandler(1, on_illegal) }.is_null() {
            signal_context::fatal(b"error: cannot install checked-trap exception handler\n");
        }
    });
}

unsafe extern "system" fn on_illegal(pointers: *mut ExceptionPointers) -> i32 {
    // SAFETY: Windows passes live exception records and a mutable saved context.
    unsafe {
        let record = &*(*pointers).record;
        let table = TABLE.load(Ordering::Acquire);
        // Software-raised exceptions and noncontinuable records are not native
        // checked instructions. Require both OS addresses to match the table PC.
        let context = &mut *(*pointers).context;
        if record.code == ILLEGAL_INSTRUCTION
            && record.flags == 0
            && record.address as usize == context.rip as usize
            && !table.is_null()
            && let Some((helper, site, option)) = lookup(table, context.rip as usize)
        {
            signal_context::enter_helper(context, helper, site as usize, option);
            return CONTINUE_EXECUTION;
        }
    }
    CONTINUE_SEARCH
}

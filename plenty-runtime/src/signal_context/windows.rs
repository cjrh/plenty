//! Windows SDK AMD64 exception ABI. Only the CONTEXT prefix through Rip is
//! accessed; the OS owns and preserves the following floating/vector state.
//! Layout cross-checked against Rust's windows_sys bindings and Zig's winnt ABI.
use std::ffi::c_void;

#[repr(C)]
pub(crate) struct Context {
    home: [u64; 6],
    flags: u32,
    mx_csr: u32,
    segments: [u16; 6],
    eflags: u32,
    debug: [u64; 6],
    rax: u64,
    pub(crate) rcx: u64,
    pub(crate) rdx: u64,
    rbx: u64,
    pub(crate) rsp: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8_to_r15: [u64; 8],
    pub(crate) rip: u64,
}
const _: () = {
    assert!(std::mem::offset_of!(Context, rcx) == 0x80);
    assert!(std::mem::offset_of!(Context, rdx) == 0x88);
    assert!(std::mem::offset_of!(Context, rsp) == 0x98);
    assert!(std::mem::offset_of!(Context, rip) == 0xf8);
};

#[repr(C)]
pub(crate) struct ExceptionRecord {
    pub(crate) code: u32,
    pub(crate) flags: u32,
    record: *mut ExceptionRecord,
    pub(crate) address: *mut c_void,
    parameters: u32,
    information: [usize; 15],
}
#[repr(C)]
pub(crate) struct ExceptionPointers {
    pub(crate) record: *mut ExceptionRecord,
    pub(crate) context: *mut Context,
}
pub(crate) const ILLEGAL_INSTRUCTION: u32 = 0xc000001d;
pub(crate) const STACK_OVERFLOW: u32 = 0xc00000fd;
pub(crate) const CONTINUE_SEARCH: i32 = 0;
pub(crate) const CONTINUE_EXECUTION: i32 = -1;

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(crate) fn AddVectoredExceptionHandler(
        first: u32,
        handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32,
    ) -> *mut c_void;
    pub(crate) fn SetThreadStackGuarantee(bytes: *mut u32) -> i32;
    fn GetStdHandle(kind: u32) -> *mut c_void;
    fn WriteFile(
        handle: *mut c_void,
        buffer: *const u8,
        bytes: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn TerminateProcess(process: *mut c_void, exit_code: u32) -> i32;
}

pub(crate) fn enter_helper(context: &mut Context, helper: usize, site: usize, option: u64) {
    context.rcx = site as u64;
    context.rdx = option;
    // Microsoft x64 requires 32 bytes of home space above the return slot.
    // The helper never returns, but still needs normal callee-entry alignment.
    context.rsp = (context.rsp & !15) - 40;
    context.rip = helper as u64;
}

/// Allocation-free termination, also usable on the guaranteed exception stack.
pub(crate) fn fatal(message: &[u8]) -> ! {
    // SAFETY: Windows supplies these handles, WriteFile reads a valid slice,
    // and a terminated process cannot resume the exhausted instruction.
    unsafe {
        let mut written = 0;
        WriteFile(
            GetStdHandle(-12i32 as u32),
            message.as_ptr(),
            message.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        );
        TerminateProcess(GetCurrentProcess(), 1);
        // A failed termination must not return into a consumed stack guard.
        libc::abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_uses_microsoft_registers_and_home_space() {
        // SAFETY: the synthetic integer-only prefix has no invalid bit patterns.
        let mut context: Context = unsafe { std::mem::zeroed() };
        context.rsp = 0x12347;
        enter_helper(&mut context, 0x56780, 0x98760, 1);
        assert_eq!(context.rip, 0x56780);
        assert_eq!(context.rsp, 0x12318);
        assert_eq!(context.rcx, 0x98760);
        assert_eq!(context.rdx, 1);
    }
}

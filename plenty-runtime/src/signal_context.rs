//! Linux GNU signal records come from libc; only register selection is local.

pub(crate) fn action(handler: usize, flags: i32) -> libc::sigaction {
    // SAFETY: zero initializes an empty signal mask and absent restorer.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler;
    action.sa_flags = flags;
    action
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn pc(context: &libc::ucontext_t) -> usize {
    context.uc_mcontext.gregs[libc::REG_RIP as usize] as usize
}
#[cfg(target_arch = "aarch64")]
pub(crate) fn pc(context: &libc::ucontext_t) -> usize {
    context.uc_mcontext.pc as usize
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn sp(context: &libc::ucontext_t) -> usize {
    context.uc_mcontext.gregs[libc::REG_RSP as usize] as usize
}
#[cfg(target_arch = "aarch64")]
pub(crate) fn sp(context: &libc::ucontext_t) -> usize {
    context.uc_mcontext.sp as usize
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn instruction_fault(context: &libc::ucontext_t, _address: usize) -> bool {
    context.uc_mcontext.gregs[libc::REG_ERR as usize] & (1 << 4) != 0
}
#[cfg(target_arch = "aarch64")]
pub(crate) fn instruction_fault(context: &libc::ucontext_t, address: usize) -> bool {
    // Instruction aborts identify the faulting instruction itself. Avoid
    // assuming x86 page-fault error bits or parsing optional ARM ESR records.
    pc(context) == address
}

pub(crate) fn enter_helper(
    context: &mut libc::ucontext_t,
    helper: usize,
    site: usize,
    option: u64,
) {
    #[cfg(target_arch = "x86_64")]
    {
        let registers = &mut context.uc_mcontext.gregs;
        registers[libc::REG_RDI as usize] = site as _;
        registers[libc::REG_RSI as usize] = option as _;
        registers[libc::REG_RSP as usize] = ((registers[libc::REG_RSP as usize] & !15) - 8) as _;
        registers[libc::REG_RIP as usize] = helper as _;
    }
    #[cfg(target_arch = "aarch64")]
    {
        let registers = &mut context.uc_mcontext;
        registers.regs[0] = site as _;
        registers.regs[1] = option;
        // AAPCS64 has no pushed return slot and requires SP % 16 == 0.
        registers.sp &= !15;
        registers.pc = helper as _;
        registers.regs[30] = 0; // This helper must never return.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_helper_receives_native_arguments_and_stack_alignment() {
        // SAFETY: no pointers in this synthetic context are dereferenced.
        let mut context: libc::ucontext_t = unsafe { std::mem::zeroed() };
        #[cfg(target_arch = "x86_64")]
        {
            context.uc_mcontext.gregs[libc::REG_RSP as usize] = 0x12340;
        }
        #[cfg(target_arch = "aarch64")]
        {
            context.uc_mcontext.sp = 0x12340;
        }
        enter_helper(&mut context, 0x56780, 0x98760, 1);
        assert_eq!(pc(&context), 0x56780);
        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(sp(&context), 0x12338);
            assert_eq!(context.uc_mcontext.gregs[libc::REG_RDI as usize], 0x98760);
            assert_eq!(context.uc_mcontext.gregs[libc::REG_RSI as usize], 1);
        }
        #[cfg(target_arch = "aarch64")]
        {
            assert_eq!(sp(&context), 0x12340);
            assert_eq!(context.uc_mcontext.regs[0], 0x98760);
            assert_eq!(context.uc_mcontext.regs[1], 1);
            assert_eq!(context.uc_mcontext.regs[30], 0);
        }
    }
}

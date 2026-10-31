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

/// The current stack frontier for a data fault. A registered AArch64 loop
/// probe moves its temporary address down while leaving the actual SP still.
pub(crate) fn fault_stack_pointer(context: &libc::ucontext_t, address: usize) -> usize {
    #[cfg(target_arch = "aarch64")]
    if crate::traps::stack_probe(pc(context)) {
        return probe_stack_pointer(context, address).unwrap_or_else(|| sp(context));
    }
    let _ = address;
    sp(context)
}

#[cfg(target_arch = "aarch64")]
fn probe_stack_pointer(context: &libc::ucontext_t, address: usize) -> Option<usize> {
    let offset = context.uc_mcontext.regs[16] as i64;
    let frame = context.uc_mcontext.regs[17];
    // The pinned backend uses 4 KiB steps, independent of kernel page size.
    // It may probe one step beyond a frame whose size is not page-aligned.
    let distance = offset.checked_neg().and_then(|n| usize::try_from(n).ok())?;
    (offset < 0
        && frame > 3 * 4096
        && frame <= u64::from(u32::MAX)
        && distance.is_multiple_of(4096)
        && distance as u64 <= frame + 4095
        && sp(context).checked_sub(distance) == Some(address))
    .then_some(address)
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

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn loop_probe_frontier_requires_negative_aligned_bounded_offset() {
        // SAFETY: no synthetic context pointers are dereferenced.
        let mut context: libc::ucontext_t = unsafe { std::mem::zeroed() };
        context.uc_mcontext.sp = 0x10_0000;
        context.uc_mcontext.regs[16] = (-0x8000i64) as u64;
        context.uc_mcontext.regs[17] = 0x9000;
        // A matching offset alone is insufficient at an unregistered PC.
        assert_eq!(fault_stack_pointer(&context, 0xf_8000), 0x10_0000);
        assert_eq!(probe_stack_pointer(&context, 0xf_8000), Some(0xf_8000));
        assert_eq!(probe_stack_pointer(&context, 0xf_7000), None);
        context.uc_mcontext.regs[17] = 0x6000;
        assert_eq!(probe_stack_pointer(&context, 0xf_8000), None);
        context.uc_mcontext.regs[17] = u64::MAX;
        assert_eq!(probe_stack_pointer(&context, 0xf_8000), None);
        context.uc_mcontext.regs[17] = 0x9000;
        for offset in [0, 0x8000, -1, i64::MIN] {
            context.uc_mcontext.regs[16] = offset as u64;
            assert_eq!(probe_stack_pointer(&context, 0xf_8000), None);
        }
    }

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

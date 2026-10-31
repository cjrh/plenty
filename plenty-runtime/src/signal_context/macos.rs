//! Darwin puts the machine state behind `uc_mcontext`, unlike Linux's inline
//! record. Only kernel-supplied signal frames (or fully backed test frames)
//! may reach these helpers. The x86-64 and ARM64 fields are libc ABI bindings.

unsafe fn machine(context: &libc::ucontext_t) -> &libc::__darwin_mcontext64 {
    // SAFETY: the caller supplies a valid, exclusively borrowed signal frame.
    unsafe {
        // SAFETY: callers only pass live signal frames with a valid machine state.
        &*context.uc_mcontext
    }
}

unsafe fn machine_mut(context: &mut libc::ucontext_t) -> &mut libc::__darwin_mcontext64 {
    // SAFETY: the caller supplies a valid, exclusively borrowed signal frame.
    unsafe {
        // SAFETY: the signal handler exclusively owns this live machine state.
        &mut *context.uc_mcontext
    }
}

pub(crate) unsafe fn pc(context: &libc::ucontext_t) -> usize {
    // SAFETY: the caller supplies a valid, exclusively borrowed signal frame.
    unsafe {
        #[cfg(target_arch = "x86_64")]
        {
            machine(context).__ss.__rip as usize
        }
        #[cfg(target_arch = "aarch64")]
        {
            machine(context).__ss.__pc as usize
        }
    }
}

pub(crate) unsafe fn sp(context: &libc::ucontext_t) -> usize {
    // SAFETY: the caller supplies a valid, exclusively borrowed signal frame.
    unsafe {
        #[cfg(target_arch = "x86_64")]
        {
            machine(context).__ss.__rsp as usize
        }
        #[cfg(target_arch = "aarch64")]
        {
            machine(context).__ss.__sp as usize
        }
    }
}

pub(crate) unsafe fn instruction_fault(context: &libc::ucontext_t, address: usize) -> bool {
    // SAFETY: the caller supplies a valid, exclusively borrowed signal frame.
    unsafe {
        #[cfg(target_arch = "x86_64")]
        {
            let _ = address;
            machine(context).__es.__err & (1 << 4) != 0
        }
        #[cfg(target_arch = "aarch64")]
        {
            let _ = address;
            // ARM64 ESR exception classes 0x20/0x21 are instruction aborts;
            // 0x24/0x25 are data aborts, even when their address equals the PC.
            matches!((machine(context).__es.__esr >> 26) & 0x3f, 0x20 | 0x21)
        }
    }
}

pub(crate) unsafe fn fault_stack_pointer(context: &libc::ucontext_t, address: usize) -> usize {
    // SAFETY: the caller supplies a valid, exclusively borrowed signal frame.
    unsafe {
        #[cfg(target_arch = "aarch64")]
        if crate::traps::stack_probe(pc(context)) {
            let registers = &machine(context).__ss;
            let offset = registers.__x[16] as i64;
            let frame = registers.__x[17];
            if let Some(distance) = offset.checked_neg().and_then(|n| usize::try_from(n).ok())
                && offset < 0
                && frame > 3 * 4096
                && frame <= u64::from(u32::MAX)
                && distance.is_multiple_of(4096)
                && distance as u64 <= frame + 4095
                && sp(context).checked_sub(distance) == Some(address)
            {
                return address;
            }
        }
        let _ = address;
        sp(context)
    }
}

pub(crate) unsafe fn enter_helper(
    context: &mut libc::ucontext_t,
    helper: usize,
    site: usize,
    option: u64,
) {
    // SAFETY: the caller supplies a valid, exclusively borrowed signal frame.
    unsafe {
        let registers = &mut machine_mut(context).__ss;
        #[cfg(target_arch = "x86_64")]
        {
            registers.__rdi = site as u64;
            registers.__rsi = option;
            registers.__rsp = (registers.__rsp & !15) - 8;
            registers.__rip = helper as u64;
        }
        #[cfg(target_arch = "aarch64")]
        {
            registers.__x[0] = site as u64;
            registers.__x[1] = option;
            registers.__sp &= !15;
            registers.__pc = helper as u64;
            registers.__lr = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_helper_receives_native_arguments_and_stack_alignment() {
        // SAFETY: uc_mcontext points to the live machine record below.
        unsafe {
            // SAFETY: all fields accept zero, and we back uc_mcontext with live storage.
            let mut machine: libc::__darwin_mcontext64 = std::mem::zeroed();
            let mut context: libc::ucontext_t = std::mem::zeroed();
            context.uc_mcontext = &raw mut machine;
            #[cfg(target_arch = "x86_64")]
            {
                machine.__ss.__rsp = 0x12340;
            }
            #[cfg(target_arch = "aarch64")]
            {
                machine.__ss.__sp = 0x12340;
            }
            enter_helper(&mut context, 0x56780, 0x98760, 1);
            assert_eq!(pc(&context), 0x56780);
            #[cfg(target_arch = "x86_64")]
            {
                assert_eq!(sp(&context), 0x12338);
                assert_eq!(machine.__ss.__rdi, 0x98760);
                assert_eq!(machine.__ss.__rsi, 1);
                machine_mut(&mut context).__es.__err = 1 << 4;
                assert!(instruction_fault(&context, 0));
            }
            #[cfg(target_arch = "aarch64")]
            {
                assert_eq!(sp(&context), 0x12340);
                assert_eq!(machine.__ss.__x[0], 0x98760);
                assert_eq!(machine.__ss.__x[1], 1);
                assert_eq!(machine.__ss.__lr, 0);
                for class in [0x20, 0x21] {
                    machine_mut(&mut context).__es.__esr = class << 26;
                    assert!(instruction_fault(&context, 0x56780));
                }
                for class in [0x24, 0x25] {
                    machine_mut(&mut context).__es.__esr = class << 26;
                    assert!(!instruction_fault(&context, 0x56780));
                }
            }
        }
    }
}

//! OS-specific signal frames; libc supplies their native ABI layouts.

#[cfg(target_os = "linux")]
#[path = "signal_context/linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::*;
#[cfg(target_os = "macos")]
#[path = "signal_context/macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;

#[cfg(unix)]
pub(crate) fn action(handler: usize, flags: i32) -> libc::sigaction {
    // SAFETY: zero initializes an empty signal mask and absent restorer.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler;
    action.sa_flags = flags;
    action
}

/// Unlike Linux, Darwin uses positive codes for user-generated signals too.
#[cfg(unix)]
pub(crate) fn hardware_fault(code: i32) -> bool {
    #[cfg(target_os = "linux")]
    {
        code > 0
    }
    #[cfg(target_os = "macos")]
    {
        (1..=8).contains(&code)
    }
}

/// The stack frontier of a fault at a registered AArch64 loop-probe store, or
/// `None` if its registers do not describe that store. The probe loop keeps
/// the negative offset from SP in x16 and the frame size in x17; each OS
/// module reads them from its own signal frame. Compiled for tests on every
/// host, since the check is pure arithmetic.
#[cfg(any(all(unix, target_arch = "aarch64"), test))]
pub(crate) fn probe_frontier(sp: usize, offset: u64, frame: u64, address: usize) -> Option<usize> {
    let offset = offset as i64;
    // The pinned backend uses 4 KiB steps, independent of kernel page size.
    // It may probe one step beyond a frame whose size is not page-aligned.
    let distance = offset.checked_neg().and_then(|n| usize::try_from(n).ok())?;
    (offset < 0
        && frame > 3 * 4096
        && frame <= u64::from(u32::MAX)
        && distance.is_multiple_of(4096)
        && distance as u64 <= frame + 4095
        && sp.checked_sub(distance) == Some(address))
    .then_some(address)
}

#[cfg(test)]
mod probe_tests {
    use super::probe_frontier;

    #[test]
    fn loop_probe_frontier_requires_negative_aligned_bounded_offset() {
        let sp = 0x10_0000;
        let offset = (-0x8000i64) as u64;
        assert_eq!(probe_frontier(sp, offset, 0x9000, 0xf_8000), Some(0xf_8000));
        assert_eq!(probe_frontier(sp, offset, 0x9000, 0xf_7000), None);
        assert_eq!(probe_frontier(sp, offset, 0x6000, 0xf_8000), None);
        assert_eq!(probe_frontier(sp, offset, u64::MAX, 0xf_8000), None);
        for offset in [0, 0x8000, -1, i64::MIN] {
            assert_eq!(probe_frontier(sp, offset as u64, 0x9000, 0xf_8000), None);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn user_generated_signals_are_not_hardware_faults() {
        assert!(!super::hardware_fault(0));
        assert!(!super::hardware_fault(-1));
        assert!(super::hardware_fault(1));
        #[cfg(target_os = "macos")]
        assert!(!super::hardware_fault(0x10001)); // Darwin SI_USER
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[path = "signal_context/windows.rs"]
mod windows;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) use windows::*;

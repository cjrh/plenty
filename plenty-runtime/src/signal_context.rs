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

pub(crate) fn action(handler: usize, flags: i32) -> libc::sigaction {
    // SAFETY: zero initializes an empty signal mask and absent restorer.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler;
    action.sa_flags = flags;
    action
}

/// Unlike Linux, Darwin uses positive codes for user-generated signals too.
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

#[cfg(test)]
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

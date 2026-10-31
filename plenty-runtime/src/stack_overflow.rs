//! Native stack exhaustion reporting.
#[cfg(unix)]
#[path = "stack_overflow/posix.rs"]
mod posix;
#[cfg(unix)]
pub(crate) use posix::guarded;
#[cfg(unix)]
#[cfg_attr(not(plenty_runtime_embedded), allow(unused_imports))]
pub(crate) use posix::install;
#[cfg(windows)]
#[path = "stack_overflow/windows.rs"]
mod windows;
#[cfg(windows)]
pub(crate) use windows::guarded;
#[cfg(windows)]
#[cfg_attr(not(plenty_runtime_embedded), allow(unused_imports))]
pub(crate) use windows::install;

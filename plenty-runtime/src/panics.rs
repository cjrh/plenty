//! Rust panic and allocation-failure handlers for compiled executables.
//!
//! std's panic handler always links its panic hook and backtrace symbolizer
//! (`gimli`, `addr2line`, `rustc_demangle`, `miniz_oxide`), even with
//! `-Cpanic=abort`, because the default hook can print a backtrace at run
//! time. The build script redirects std's `rust_begin_unwind` and
//! `__rust_alloc_error_handler` to these functions with `--defsym` in the
//! executable's link arguments, so the linker drops std's versions (issue
//! #503). A Rust panic here is a runtime defect; Plenty's own runtime errors
//! go through the trap handlers and are unaffected.

use std::ffi::c_void;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

unsafe extern "C" {
    fn write(descriptor: i32, bytes: *const c_void, count: usize) -> isize;
    fn abort() -> !;
}

static PANICKING: AtomicBool = AtomicBool::new(false);

/// Writes straight to standard error: no lock, buffer, or allocation, so it
/// works whatever state the panicking code left std's streams in.
struct Stderr;

impl std::fmt::Write for Stderr {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let mut rest = text.as_bytes();
        while !rest.is_empty() {
            // SAFETY: `rest` is a live byte slice.
            let written = unsafe { write(2, rest.as_ptr().cast(), rest.len()) };
            if written <= 0 {
                return Err(std::fmt::Error);
            }
            rest = &rest[written as usize..];
        }
        Ok(())
    }
}

/// Replaces std's `rust_begin_unwind`, with the same signature.
#[unsafe(no_mangle)]
extern "Rust" fn plenty_rust_panic(info: &core::panic::PanicInfo<'_>) -> ! {
    // A panic while reporting a panic aborts without a second message.
    if !PANICKING.swap(true, Ordering::Relaxed) {
        // Standard error is the only report channel, so a failed write is
        // ignored.
        let mut out = Stderr;
        let _ = out.write_str("error: internal runtime panic");
        if let Some(at) = info.location() {
            let _ = write!(out, " at {}:{}:{}", at.file(), at.line(), at.column());
        }
        let _ = writeln!(out, ": {}", info.message());
    }
    // SAFETY: abort has no preconditions.
    unsafe { abort() }
}

/// Replaces std's `__rust_alloc_error_handler`, with the same signature.
#[unsafe(no_mangle)]
extern "Rust" fn plenty_rust_alloc_error(size: usize, _align: usize) -> ! {
    // Inside an arena call the failure is the arena's: the call reports it as
    // an allocation error instead.
    crate::actor_heap::abandon_arena_call();
    let _ = writeln!(Stderr, "error: memory allocation of {size} bytes failed");
    // Inside an actor handler this is the handler's failure, usually its heap
    // limit: contain it like a trap.
    crate::actor_heap::recover(crate::actor_heap::TrapKind::Memory);
    // SAFETY: abort has no preconditions.
    unsafe { abort() }
}

//! Standalone unit tests for the fault boundary, usable before the full runtime
//! builds on a new host. Pass a matching libc rlib with `rustc --test --extern`.
//! Cross-target type checks can use `--emit=metadata` and libc's rmeta instead.
#![allow(dead_code)]
#![cfg(any(target_os = "linux", target_os = "macos"))]

#[repr(C)]
struct Site;

// Trap-table tests compare helper addresses without invoking a diagnostic.
// Keep those symbols local to the harness; production uses src/io.rs.
mod io {
    macro_rules! helpers {
        ($($name:ident),*) => { $(pub extern "C" fn $name() { unreachable!() })* };
    }
    helpers!(
        plenty_trap_overflow,
        plenty_trap_div_zero,
        plenty_trap_shift,
        plenty_trap_cast_range,
        plenty_trap_unwrap
    );
}

#[path = "../src/signal_context.rs"]
mod signal_context;
#[path = "../src/stack_overflow.rs"]
mod stack_overflow;
#[path = "../src/traps.rs"]
mod traps;

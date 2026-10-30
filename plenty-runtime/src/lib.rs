//! Native support for compiled Plenty programs.
//!
//! ABI contract: generated code passes live, correctly typed pointers and owns
//! every managed operand. Helpers borrow arguments and return one owned result.
//! Only the compiler may construct descriptors or call the opcode dispatcher.
//! No Rust reference, Vec, String, or enum layout crosses this boundary.
//! Raw slots may contain zero only while uninitialized/moved or being destroyed.
//! Only compiler-checked jobs may execute on native worker threads.
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "allocation-checks")]
mod accounting;
mod actor_heap;
mod aggregates;
mod channels;
mod clist_layout;
mod closures;
mod collection_opcodes;
mod control;
mod deadline;
mod entries;
mod entropy;
mod entry_order;
mod executors;
mod files;
mod formatting;
mod generators;
mod io;
mod libraries;
mod list_layout;
mod math;
mod memory;
mod mutex;
mod numbers;
mod open_modes;
mod panics;
mod ranges;
mod regex;
mod render_buffer;
mod signal_context;
mod signals;
mod stack_overflow;
mod strings;
#[cfg(test)]
mod tests;
mod text_io;
mod text_lines;
mod threads;
mod time;
mod traps;
mod unicode;
// Upstream vendor files retain their original crate-relative imports.
pub(crate) use unicode::vendor::{lookups, perfect_hash, quick_check, tables};

fn fail(message: &str) -> ! {
    fail_kind(actor_heap::TrapKind::Other, message)
}

/// `fail` for a trap that `std.actor` reports by kind.
fn fail_kind(kind: actor_heap::TrapKind, message: &str) -> ! {
    io::flush();
    eprintln!("error: {message}");
    actor_heap::recover(kind);
    std::process::exit(1)
}

/// Source position of an operation that can trap. The compiler emits these
/// records as read-only data in the program's object and passes a pointer to
/// the helper that may trap; see "Traps" in the errors reference.
///
/// `file` is the distance in bytes from this record to the file's name, a
/// `u32` length followed by that many UTF-8 bytes, or zero without a file. A
/// relative offset keeps the table free of relocations.
#[repr(C)]
pub(crate) struct Site {
    line: u32,
    column: u32,
    file: u32,
    standard_library: u32,
}

thread_local! {
    // Records live in immutable object data, so this never owns an allocation
    // and cannot dangle when a tail call discards its stack frame.
    static CALLER: std::cell::Cell<*const Site> = const { std::cell::Cell::new(std::ptr::null()) };
}

#[unsafe(no_mangle)]
extern "C" fn plenty_caller_enter(site: *const Site) -> *const Site {
    CALLER.with(|caller| {
        let old = caller.get();
        if !site.is_null() {
            caller.set(site);
        }
        old
    })
}

#[unsafe(no_mangle)]
extern "C" fn plenty_caller_leave(site: *const Site) {
    CALLER.with(|caller| caller.set(site));
}

/// Restores the caller context when dropped. The runtime holds one around
/// each call into compiled code that it makes on the caller's thread: class
/// destructors, generator resumption, and key-sort comparators. That code may
/// tail-call std and so return with its own site in the context. With these
/// scopes no runtime helper changes the context, so standard-library code
/// calls helpers without saving and restoring it (issue #366).
pub(crate) struct CallerScope(*const Site);

impl CallerScope {
    pub(crate) fn save() -> Self {
        Self(CALLER.with(std::cell::Cell::get))
    }
}

impl Drop for CallerScope {
    fn drop(&mut self) {
        CALLER.with(|caller| caller.set(self.0));
    }
}

/// `file:line:column`, or `line:column` without a file.
struct Position(*const Site);

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // SAFETY: a Position holds a non-null record of the compiler's site
        // table, whose file offset, when nonzero, leads to a length-prefixed
        // UTF-8 name in the same data object.
        unsafe {
            let at = &*self.0;
            if at.file == 0 {
                return write!(f, "{}:{}", at.line, at.column);
            }
            let name = self.0.cast::<u8>().add(at.file as usize);
            let len = name.cast::<u32>().read_unaligned() as usize;
            let name = std::slice::from_raw_parts(name.add(4), len);
            write!(
                f,
                "{}:{}:{}",
                std::str::from_utf8_unchecked(name),
                at.line,
                at.column
            )
        }
    }
}

fn caller() -> Option<Position> {
    let caller = CALLER.with(std::cell::Cell::get);
    (!caller.is_null()).then_some(Position(caller))
}

/// Writes `file:line:column: ` of the nearest non-std caller on this thread to
/// standard error and returns 1, or writes nothing and returns 0 when there is
/// none. Standard-library code reaches it as `__caller_location`; see "Caller
/// locations" in the errors reference. Formats straight to standard error, so
/// it never allocates.
#[unsafe(no_mangle)]
extern "C" fn plenty_write_caller_location() -> u8 {
    match caller() {
        Some(position) => {
            use std::io::Write;
            drop(write!(std::io::stderr().lock(), "{position}: "));
            1
        }
        None => 0,
    }
}

/// Traps with `error: file:line:column: message`. A null site prints the bare
/// message: synthesized operations and the token front end have no position.
///
/// The message is formatted straight to standard error, so a trap never
/// allocates.
fn fail_at(site: *const Site, message: std::fmt::Arguments) -> ! {
    fail_kind_at(actor_heap::TrapKind::Other, site, message)
}

/// `fail_at` for a trap that `std.actor` reports by kind.
fn fail_kind_at(kind: actor_heap::TrapKind, site: *const Site, message: std::fmt::Arguments) -> ! {
    io::flush();
    if site.is_null() {
        eprintln!("error: {message}");
    } else {
        eprintln!("error: {}: {message}", Position(site));
    }
    // SAFETY: a non-null site is a record in the compiler's site table.
    if unsafe { site.as_ref() }.is_some_and(|at| at.standard_library != 0)
        && let Some(caller) = caller()
    {
        eprintln!("note: called from {caller}");
    }
    actor_heap::recover(kind);
    std::process::exit(1)
}

// Cargo tests provide their own entry point. The compiler's packaged archive
// instead exports the system entry point and calls the Cranelift object.
#[cfg(plenty_runtime_embedded)]
unsafe extern "C" {
    fn plenty_main() -> i32;
}

#[cfg(plenty_runtime_embedded)]
#[unsafe(no_mangle)]
/// Native entry point for a compiled Plenty executable.
///
/// # Safety
/// The linked object must supply a valid `plenty_main` with the declared ABI.
/// `argv` must contain `argc` valid process-lifetime, NUL-terminated arguments.
pub unsafe extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
    // SAFETY: the system supplies process-lifetime argument pointers.
    unsafe {
        text_io::set_arguments(argc, argv);
    }
    #[cfg(feature = "allocation-checks")]
    accounting::start();
    stack_overflow::install();
    // SAFETY: the final executable supplies the generated zero-argument entry.
    let status = stack_overflow::guarded(|| unsafe { plenty_main() });
    io::flush();
    #[cfg(feature = "allocation-checks")]
    accounting::finish();
    status
}

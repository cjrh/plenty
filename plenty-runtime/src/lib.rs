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
mod aggregates;
mod channels;
mod closures;
mod control;
mod deadline;
mod entries;
mod entry_order;
mod executors;
mod files;
mod generators;
mod io;
mod libraries;
mod math;
mod memory;
mod numbers;
mod open_modes;
mod ranges;
mod render_buffer;
mod stack_overflow;
mod strings;
#[cfg(test)]
mod tests;
mod text_io;
mod text_lines;
mod threads;
mod time;
mod unicode;
// Upstream vendor files retain their original crate-relative imports.
pub(crate) use unicode::vendor::{lookups, perfect_hash, quick_check, tables};

fn fail(message: &str) -> ! {
    io::flush();
    eprintln!("error: {message}");
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
}

/// Traps with `error: file:line:column: message`. A null site prints the bare
/// message: synthesized operations and the token front end have no position.
///
/// The message is formatted straight to standard error, so a trap never
/// allocates.
fn fail_at(site: *const Site, message: std::fmt::Arguments) -> ! {
    io::flush();
    // SAFETY: a non-null site is a record in the compiler's site table, whose
    // file offset, when nonzero, leads to a length-prefixed UTF-8 name in the
    // same data object.
    unsafe {
        match site.as_ref() {
            None => eprintln!("error: {message}"),
            Some(at) if at.file == 0 => {
                eprintln!("error: {}:{}: {message}", at.line, at.column)
            }
            Some(at) => {
                let name = site.cast::<u8>().add(at.file as usize);
                let len = name.cast::<u32>().read_unaligned() as usize;
                let name = std::slice::from_raw_parts(name.add(4), len);
                eprintln!(
                    "error: {}:{}:{}: {message}",
                    std::str::from_utf8_unchecked(name),
                    at.line,
                    at.column
                )
            }
        }
    }
    std::process::exit(1)
}

// Cargo tests provide their own entry point. The compiler's packaged archive
// instead exports the system entry point and calls the Cranelift object.
#[cfg(plenty_runtime_embedded)]
extern "C" {
    fn plenty_main() -> i32;
}

#[cfg(plenty_runtime_embedded)]
#[no_mangle]
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

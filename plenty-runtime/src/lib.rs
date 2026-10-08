//! Native support for compiled Plenty programs.
//!
//! ABI contract: generated code passes live, correctly typed pointers and owns
//! every managed operand. Helpers borrow arguments and return one owned result.
//! Only the compiler may construct descriptors or call the opcode dispatcher.
//! No Rust reference, Vec, String, or enum layout crosses this boundary.
//! Raw slots may contain zero only while uninitialized/moved or being destroyed.
//! Execution is single-threaded; native callbacks may reenter the runtime.
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "allocation-checks")]
mod accounting;
mod aggregates;
mod closures;
mod entries;
mod files;
mod generators;
mod io;
mod libraries;
mod memory;
mod numbers;
mod ranges;
mod render_buffer;
mod strings;
#[cfg(test)]
mod tests;
mod text_io;
mod text_lines;

fn fail(message: &str) -> ! {
    io::flush();
    eprintln!("error: {message}");
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
    // SAFETY: the final executable supplies the generated zero-argument entry.
    let status = unsafe { plenty_main() };
    io::flush();
    #[cfg(feature = "allocation-checks")]
    accounting::finish();
    status
}

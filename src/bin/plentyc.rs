//! `plentyc`: the single-file compiler, without package management.
use std::process::ExitCode;

#[path = "../compiler_cli.rs"]
mod compiler_cli;

// mimalloc rather than the system allocator: see the dependency in
// Cargo.toml (issue #315).
#[cfg(not(feature = "profile-alloc"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

// With `profile-alloc`, every allocation is counted against the measured
// function that makes it (issue #150).
#[cfg(feature = "profile-alloc")]
#[global_allocator]
static ALLOCATOR: hotpath::CountingAllocator<mimalloc::MiMalloc> =
    hotpath::CountingAllocator::with(mimalloc::MiMalloc);

fn main() -> ExitCode {
    pretty_env_logger::init();
    match compiler_cli::run("plentyc", std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

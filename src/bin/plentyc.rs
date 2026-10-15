//! `plentyc`: the single-file compiler, without package management.
use std::process::ExitCode;

#[path = "../compiler_cli.rs"]
mod compiler_cli;

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

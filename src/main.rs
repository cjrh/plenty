//! Plenty's AOT compiler and compile-and-run command.
use std::error::Error;
use std::path::Path;
use std::process::{Command, ExitCode, ExitStatus};

const USAGE: &str = "\
Usage: plenty FILE
       plenty --check FILE
       plenty --compile FILE -o OUT
       plenty -h | --help

FILE: compile to a temporary executable and run it.
Programs require def main() -> () or def main() -> i32.
--check: parse and type-check without executing the program.
--compile: emit a native executable with Cranelift and the system cc linker.
Running and compiling require the system linker driver cc on PATH.
--legacy before FILE or --compile selects the historical stack syntax.
";

fn main() -> ExitCode {
    pretty_env_logger::init();
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let legacy = args.first().is_some_and(|s| s == "--legacy");
    if legacy {
        args.remove(0);
    }
    match args.as_slice() {
        [] if !legacy => {
            print!("{USAGE}");
        }
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{USAGE}");
        }
        [flag, source] if flag == "--check" && !legacy => {
            plenty::check_source(&read_source(source)?)?;
        }
        [flag, source, option, output]
            if flag == "--compile" && (option == "-o" || option == "--output") =>
        {
            compile(&read_source(source)?, Path::new(output), legacy)?;
        }
        [source] if !source.starts_with('-') => {
            let source = read_source(source)?;
            let workspace = tempfile::tempdir()?;
            let executable = workspace
                .path()
                .join(format!("program{}", std::env::consts::EXE_SUFFIX));
            compile(&source, &executable, legacy)?;
            // Inherit stdin, stdout, stderr, environment, and working directory.
            // Keep the temporary directory alive until the child has exited.
            let status = Command::new(&executable).status()?;
            return Ok(exit_code(status));
        }
        _ => return Err(format!("unrecognised arguments\n{USAGE}").into()),
    }
    Ok(ExitCode::SUCCESS)
}

fn compile(source: &str, output: &Path, legacy: bool) -> Result<(), Box<dyn Error>> {
    if legacy {
        plenty::compile_legacy_source_to_executable(source, output)
    } else {
        plenty::compile_source_to_executable(source, output)
    }
}

fn read_source(path: &str) -> Result<String, Box<dyn Error>> {
    std::fs::read_to_string(path).map_err(|error| format!("reading {path}: {error}").into())
}

fn exit_code(status: ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return ExitCode::from(code as u8);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ExitCode::from((128 + signal) as u8);
        }
    }
    ExitCode::FAILURE
}

//! Plenty's AOT compiler and compile-and-run command.
use std::error::Error;
use std::path::Path;
use std::process::{Command, ExitCode, ExitStatus};

const USAGE: &str = "\
Usage: plenty FILE
       plenty --check FILE
       plenty --check-module FILE
       plenty --compile FILE -o OUT
       plenty --emit-object FILE -o OUT
       plenty --emit-runtime DIR
       plenty --print-target
       plenty -h | --help

FILE: compile to a temporary executable and run it.
Programs require def main() -> () or def main() -> i32.
--module-root DIR: resolve absolute imports here (default: FILE's directory).
--check: parse and type-check without executing the program.
--check-module: check a library module without requiring main.
--compile: emit a native executable with Cranelift.
--emit-object: emit an application object without linking (requires main).
--emit-runtime: extract the matching runtime archive and native link arguments.
--linker PATH: cc-compatible linker driver (default: cc on PATH).
--link-arg ARG: pass one extra driver argument verbatim; may be repeated.
--target TRIPLE: require this target to match the supported packaged runtime.
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
    let mut options = plenty::CompileOptions::default();
    let mut link_options = false;
    let mut index = 0;
    while index < args.len() {
        if matches!(args[index].as_str(), "--linker" | "--link-arg" | "--target") {
            let flag = args.remove(index);
            if index == args.len() {
                return Err(format!("{flag} requires a value").into());
            }
            let value = args.remove(index);
            if flag == "--target" {
                options.target = Some(value);
                continue;
            } else if flag == "--linker" {
                if value.is_empty() {
                    return Err("--linker requires a nonempty path".into());
                }
                options.linker = value.into();
            } else {
                options.link_args.push(value.into());
            }
            link_options = true;
        } else {
            index += 1;
        }
    }
    let root = if let Some(index) = args.iter().position(|a| a == "--module-root") {
        if legacy {
            return Err("--module-root is not supported with --legacy".into());
        }
        args.remove(index);
        if index == args.len() {
            return Err("--module-root requires a directory".into());
        }
        Some(std::path::PathBuf::from(args.remove(index)))
    } else {
        None
    };
    if options.target.is_some() {
        plenty::validate_target(options.target.as_deref())?;
    }
    if link_options
        && (args.is_empty()
            || args.first().is_some_and(|a| {
                matches!(
                    a.as_str(),
                    "--check"
                        | "--check-module"
                        | "--help"
                        | "-h"
                        | "--emit-object"
                        | "--emit-runtime"
                )
            }))
    {
        return Err("linker options require compilation or execution".into());
    }
    match args.as_slice() {
        [flag] if flag == "--print-target" && !legacy && root.is_none() && !link_options => {
            println!("{}", plenty::native_target());
        }
        [flag, directory] if flag == "--emit-runtime" && !legacy && root.is_none() => {
            plenty::emit_runtime(Path::new(directory))?;
        }
        [flag, source, option, output]
            if flag == "--emit-object" && !legacy && (option == "-o" || option == "--output") =>
        {
            plenty::compile_file_to_object(Path::new(source), Path::new(output), root.as_deref())?;
        }
        [] if !legacy => {
            print!("{USAGE}");
        }
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{USAGE}");
        }
        [flag, source] if flag == "--check" && !legacy => {
            plenty::check_file(Path::new(source), root.as_deref())?;
        }
        [flag, source] if flag == "--check-module" && !legacy => {
            plenty::check_module_file(Path::new(source), root.as_deref())?;
        }
        [flag, source, option, output]
            if flag == "--compile" && (option == "-o" || option == "--output") =>
        {
            compile(
                Path::new(source),
                Path::new(output),
                legacy,
                root.as_deref(),
                &options,
            )?;
        }
        [source] if !source.starts_with('-') => {
            let workspace = tempfile::tempdir()?;
            let executable = workspace
                .path()
                .join(format!("program{}", std::env::consts::EXE_SUFFIX));
            compile(
                Path::new(source),
                &executable,
                legacy,
                root.as_deref(),
                &options,
            )?;
            // Inherit stdin, stdout, stderr, environment, and working directory.
            // Keep the temporary directory alive until the child has exited.
            let status = Command::new(&executable).status()?;
            return Ok(exit_code(status));
        }
        _ => return Err(format!("unrecognised arguments\n{USAGE}").into()),
    }
    Ok(ExitCode::SUCCESS)
}

fn compile(
    source: &Path,
    output: &Path,
    legacy: bool,
    root: Option<&Path>,
    options: &plenty::CompileOptions,
) -> Result<(), Box<dyn Error>> {
    if legacy {
        plenty::compile_legacy_source_to_executable_with_options(
            &read_source(source)?,
            output,
            options,
        )
    } else {
        plenty::compile_file_to_executable_with_options(source, output, root, options)
    }
}

fn read_source(path: &Path) -> Result<String, Box<dyn Error>> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("reading {}: {error}", path.display()).into())
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

//! `plenty`: project commands over `plenty.toml`, plus the single-file
//! compiler interface that `plentyc` also provides.
use plenty::project::{self, BuildOptions, Profile, Template};
use std::error::Error;
use std::path::Path;
use std::process::ExitCode;

mod compiler_cli;

const PROJECT_USAGE: &str = "\
Project commands (run inside a directory containing plenty.toml or below it):
       plenty new PATH [--lib] [--name NAME]
       plenty init [--lib] [--name NAME]
       plenty check [--locked]
       plenty build [--release] [--locked]
       plenty run [--release] [--locked] [--bin NAME] [-- ARGS...]
       plenty test [--release] [--locked]
       plenty clean

new / init: create a package with src/main.plenty (or src/lib.plenty with --lib).
check: type-check every target. build: compile executables into target/debug,
or target/release with --release. run: build one executable and run it with
ARGS. test: compile and run each tests/*.plenty program; nonzero exit fails.
--locked: fail instead of creating or updating plenty.lock.
A file whose name matches a command runs as ./NAME.

Single-file compiler (also available as plentyc):
";

fn main() -> ExitCode {
    pretty_env_logger::init();
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<ExitCode, Box<dyn Error>> {
    let Some(command) = args.first() else {
        print!("{PROJECT_USAGE}{}", compiler_cli::usage("plenty"));
        return Ok(ExitCode::SUCCESS);
    };
    if matches!(command.as_str(), "-h" | "--help") && args.len() == 1 {
        print!("{PROJECT_USAGE}{}", compiler_cli::usage("plenty"));
        return Ok(ExitCode::SUCCESS);
    }
    let rest = &args[1..];
    let here = Path::new(".");
    match command.as_str() {
        "new" => {
            let (template, name, positional) = template_options(rest)?;
            let [path] = positional.as_slice() else {
                return Err("usage: plenty new PATH [--lib] [--name NAME]".into());
            };
            project::new(Path::new(path), name.as_deref(), template)?;
            eprintln!("   Created package at {path}");
        }
        "init" => {
            let (template, name, positional) = template_options(rest)?;
            if !positional.is_empty() {
                return Err("usage: plenty init [--lib] [--name NAME]".into());
            }
            project::init(here, name.as_deref(), template)?;
            eprintln!("   Created package in the current directory");
        }
        "check" => {
            let (options, _, _) = build_options(rest, false, false)?;
            project::check(here, &options)?;
        }
        "build" => {
            let (options, _, _) = build_options(rest, true, false)?;
            for executable in project::build(here, &options)? {
                eprintln!("  Finished {}", executable.display());
            }
        }
        "run" => {
            let (options, binary, arguments) = build_options(rest, true, true)?;
            return project::run(here, binary.as_deref(), &arguments, &options);
        }
        "test" => {
            let (options, _, _) = build_options(rest, true, false)?;
            let summary = project::test(here, &options)?;
            if !summary.failed.is_empty() {
                return Ok(ExitCode::FAILURE);
            }
        }
        "clean" => {
            if !rest.is_empty() {
                return Err("usage: plenty clean".into());
            }
            project::clean(here)?;
        }
        _ => return compiler_cli::run("plenty", args),
    }
    Ok(ExitCode::SUCCESS)
}

fn template_options(args: &[String]) -> Result<(Template, Option<String>, Vec<String>), String> {
    let mut template = Template::Application;
    let mut name = None;
    let mut positional = vec![];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lib" => template = Template::Library,
            "--name" => name = Some(args.next().ok_or("--name requires a value")?.clone()),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ => positional.push(arg.clone()),
        }
    }
    Ok((template, name, positional))
}

/// Parse build flags. Arguments after `--` belong to the program for `run`.
fn build_options(
    args: &[String],
    profiles: bool,
    running: bool,
) -> Result<(BuildOptions, Option<String>, Vec<String>), String> {
    let mut options = BuildOptions::default();
    let mut binary = None;
    let mut forwarded = vec![];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--release" if profiles => options.profile = Profile::Release,
            "--locked" => options.locked = true,
            "--bin" if running => {
                binary = Some(args.next().ok_or("--bin requires a name")?.clone());
            }
            "--" if running => {
                forwarded = args.cloned().collect();
                break;
            }
            other => {
                let hint = if running {
                    "; pass program arguments after --"
                } else {
                    ""
                };
                return Err(format!("unexpected argument {other:?}{hint}"));
            }
        }
    }
    Ok((options, binary, forwarded))
}

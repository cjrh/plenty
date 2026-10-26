//! `plenty`: project commands over `plenty.toml`, plus the single-file
//! compiler interface that `plentyc` also provides.
use plenty::project::manifest::{Dependency, GitSelector, Source, VersionReq};
use plenty::project::{self, BuildOptions, Profile, Template, TestOptions};
use std::error::Error;
use std::path::Path;
use std::process::ExitCode;

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

const PROJECT_USAGE: &str = "\
Project commands (run inside a directory containing plenty.toml or below it):
       plenty new PATH [--lib] [--name NAME]
       plenty init [--lib] [--name NAME]
       plenty check [--locked] [--offline] [--check-all-methods]
       plenty build [--release] [--no-strip] [--locked] [--offline]
       plenty run [--release] [--no-strip] [--locked] [--offline] [--bin NAME] [-- ARGS...]
       plenty test [--release] [--locked] [--offline] [--no-capture] [--seed N] [--list]
                   [--jobs N] [--timeout SECONDS] [FILTER...]
       plenty add [ALIAS] (--path DIR | --git URL (--tag TAG | --rev COMMIT) [--subdir DIR])
                  [--version REQ] [--offline]
       plenty fetch [--locked] [--offline]
       plenty update [PACKAGE...] [--offline]
       plenty graph [--locked] [--offline]
       plenty clean
       plenty cache (stats | clean)
       plenty lsp

new / init: create a package with src/main.plenty (or src/lib.plenty with --lib).
check: type-check every target; --check-all-methods also checks every method
of every generic instance, including methods nothing calls. build: compile executables into target/debug,
or target/release with --release. run: build one executable and run it with
ARGS. test: run each test_* function in tests/*.plenty in its own process, or
each tests/*.plenty program that defines main; a FILTER keeps the tests whose
FILE::NAME contains it. Output is shown only for failing tests unless
--no-capture. --seed N sets PLENTY_TEST_SEED for std.test.rng. --jobs N runs at
most N tests at once (default: one per CPU). --timeout SECONDS fails a test
that runs longer.
add: add a dependency to plenty.toml and lock it. fetch: download and lock
every dependency without compiling. update: move Git dependencies (all, or the
named packages) to the commits their tags and revisions name now. graph: print
the package graph file that plentyc --package-graph reads.
In a workspace root these commands act on every member.
cache: report or empty the compiled-function cache (PLENTY_CACHE_DIR,
default ~/.cache/plenty; PLENTY_CACHE=off disables it).
lsp: serve the Language Server Protocol on stdin and stdout for an editor:
diagnostics, hover, go to definition, and completion.
--no-strip: keep the symbol table and debug sections in built executables.
--locked: fail instead of creating or updating plenty.lock.
--offline: never fetch; use only the package cache in .pkg/.
A file whose name matches a command runs as ./NAME.

Single-file compiler (also available as plentyc):
";

fn main() -> ExitCode {
    pretty_env_logger::init();
    #[cfg(feature = "profile")]
    // SAFETY: startup has not spawned any threads.
    let _profile = unsafe { plenty::profile::start() };
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
            let mut rest = rest.to_vec();
            let mut check = plenty::CheckOptions::default();
            if let Some(index) = rest.iter().position(|a| a == "--check-all-methods") {
                rest.remove(index);
                check.all_methods = true;
            }
            let (mut options, _, _) = build_options(&rest, false, false)?;
            options.check = check;
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
            let options = test_options(rest)?;
            let summary = project::test(here, &options)?;
            if !summary.failed.is_empty() {
                return Ok(ExitCode::FAILURE);
            }
        }
        "add" => {
            let (alias, dependency, offline) = add_options(rest)?;
            let alias = project::add(here, alias.as_deref(), dependency, offline)?;
            eprintln!("{:>10} dependency `{alias}`", "Added");
        }
        "fetch" => {
            let (options, _, _) = build_options(rest, false, false)?;
            project::fetch(here, &options)?;
        }
        "update" => {
            let mut offline = false;
            let mut packages = vec![];
            for arg in rest {
                match arg.as_str() {
                    "--offline" => offline = true,
                    flag if flag.starts_with('-') => {
                        return Err(format!("unknown option {flag}").into());
                    }
                    name => packages.push(name.to_owned()),
                }
            }
            for (name, old, new) in project::update(here, &packages, offline)? {
                if old.is_empty() {
                    eprintln!("{:>10} {name} at {new}", "Locking");
                } else {
                    eprintln!("{:>10} {name} {old} -> {new}", "Updating");
                }
            }
        }
        "graph" => {
            let (options, _, _) = build_options(rest, false, false)?;
            print!("{}", project::graph(here, &options)?);
        }
        "clean" => {
            if !rest.is_empty() {
                return Err("usage: plenty clean".into());
            }
            project::clean(here)?;
        }
        "cache" => return cache_command(rest),
        "lsp" => {
            if !rest.is_empty() {
                return Err("usage: plenty lsp".into());
            }
            plenty::lsp::serve()?;
        }
        _ => return compiler_cli::run("plenty", args),
    }
    Ok(ExitCode::SUCCESS)
}

/// `plenty cache stats` and `plenty cache clean`, on the directory the
/// environment selects.
fn cache_command(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    use plenty::cache::{self, CacheMode};
    let mode = CacheMode::Environment;
    match args {
        [command] if command == "stats" => match cache::summary(&mode) {
            Some(summary) => println!("{summary}"),
            None => println!("compile cache: off"),
        },
        [command] if command == "clean" => match cache::clean(&mode)? {
            Some((entries, bytes)) => eprintln!("   Removed {entries} entries, {bytes} bytes"),
            None => println!("compile cache: off"),
        },
        _ => return Err("usage: plenty cache (stats | clean)".into()),
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

fn test_options(args: &[String]) -> Result<TestOptions, String> {
    let mut options = TestOptions::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--release" => options.build.profile = Profile::Release,
            "--locked" => options.build.locked = true,
            "--offline" => options.build.offline = true,
            "--no-capture" => options.no_capture = true,
            "--list" => options.list = true,
            "--seed" => {
                let value = args.next().ok_or("--seed requires a value")?;
                options.seed = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--seed {value:?} is not a u64"))?,
                );
            }
            "--jobs" => {
                let value = args.next().ok_or("--jobs requires a value")?;
                options.jobs = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--jobs {value:?} is not a positive count"))?,
                );
            }
            "--timeout" => {
                let value = args.next().ok_or("--timeout requires a value")?;
                let seconds: u64 = value
                    .parse()
                    .ok()
                    .filter(|&seconds| seconds > 0)
                    .ok_or_else(|| {
                        format!("--timeout {value:?} is not a positive number of seconds")
                    })?;
                options.timeout = Some(std::time::Duration::from_secs(seconds));
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            filter => options.filters.push(filter.to_owned()),
        }
    }
    Ok(options)
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
            "--no-strip" if profiles => options.compile.strip = false,
            "--locked" => options.locked = true,
            "--offline" => options.offline = true,
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

const ADD_USAGE: &str = "usage: plenty add [ALIAS] (--path DIR | --git URL (--tag TAG | --rev COMMIT) [--subdir DIR]) [--version REQ] [--offline]";

/// Parse `plenty add`: the alias, the dependency, and `--offline`.
fn add_options(args: &[String]) -> Result<(Option<String>, Dependency, bool), String> {
    let mut alias = None;
    let (mut path, mut url, mut tag, mut rev, mut subdir, mut version) =
        (None, None, None, None, None, None);
    let mut offline = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |slot: &mut Option<String>| -> Result<(), String> {
            let value = args.next().ok_or(format!("{arg} requires a value"))?;
            if slot.replace(value.clone()).is_some() {
                return Err(format!("{arg} is given twice"));
            }
            Ok(())
        };
        match arg.as_str() {
            "--path" => value(&mut path)?,
            "--git" => value(&mut url)?,
            "--tag" => value(&mut tag)?,
            "--rev" => value(&mut rev)?,
            "--subdir" => value(&mut subdir)?,
            "--version" => value(&mut version)?,
            "--offline" => offline = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            name if alias.is_none() => alias = Some(name.to_owned()),
            _ => return Err(ADD_USAGE.into()),
        }
    }
    let source = match (path, url, tag, rev) {
        (Some(path), None, None, None) if subdir.is_none() => Source::Path(path.into()),
        (None, Some(url), Some(tag), None) => Source::Git {
            url,
            subdir,
            selector: GitSelector::Tag(tag),
        },
        (None, Some(url), None, Some(rev)) => Source::Git {
            url,
            subdir,
            selector: GitSelector::Rev(rev),
        },
        _ => return Err(ADD_USAGE.into()),
    };
    let version = match version {
        None => None,
        Some(text) => Some(VersionReq::parse(&text).ok_or(format!(
            "--version {text:?} must be X.Y.Z, ^X.Y.Z, =X.Y.Z, or >=X.Y.Z"
        ))?),
    };
    Ok((alias, Dependency { source, version }, offline))
}

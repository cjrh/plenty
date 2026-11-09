//! The single-file compiler interface shared by `plentyc` and `plenty`.
//! Included by path into both binaries so each keeps its own `main`.
use plenty::project::exit_code;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, ExitCode};

/// Compiler usage; `{0}` is replaced with the program name.
const USAGE: &str = "\
Usage: {0} FILE
       {0} --check FILE
       {0} --check-module FILE
       {0} --compile FILE -o OUT
       {0} --compile FILE --dist DIR
       {0} --emit-object FILE -o OUT
       {0} --dump STAGES FILE -o DIR [--dump-std] [--dump-filter TEXT]
       {0} --fuzz FILE -o OUT [--libfuzzer PATH]
       {0} --shared-library FILE --library-name NAME -o OUT
       {0} --static-library FILE --library-name NAME -o OUT
       {0} --runtime-interface FILE --library-name NAME -o OUT.plentyi
       {0} --extract-interface LIBRARY --library-name NAME -o OUT
       {0} --verify-interface LIBRARY INTERFACE
       {0} --emit-runtime DIR
       {0} --print-target
       {0} -V | --version
       {0} -h | --help

FILE: source files use .plenty or .p; interfaces use .plentyi or .pi.
Reuse a cached executable or compile/link, then run it privately.
Programs require main() returning (), i32, Result[(), E], or Result[i32, E].
--module-root DIR: resolve absolute imports here (default: FILE's directory).
--package-graph GRAPH: resolve imports through a package graph file, as
  `plenty graph` writes it, for FILE, --check, --check-module, or --compile.
--check: parse and type-check without executing the program.
--check-module: check a library module without requiring main.
--check-all-methods: with --check or --check-module, also check every method
  of every generic class and enum instance the program creates, including
  methods nothing calls. By default an instance's method is checked only
  where it is used.
--compile: emit a native executable with Cranelift.
--dist DIR: publish a new application directory containing FILE's stem executable,
  reached native engines and their packaged notices. Uses application-local engine lookup.
  Requires --compile; excludes -o and --link-arg. Not a static build.
--emit-object: emit an application object without linking (requires main).
--dump: write each function's compiler output as text to DIR (`-o -` prints
  it): index.txt, then one file per stage. STAGES is `all` or a comma list of
  ops, clif, clif-opt, asm. Bypasses both caches; links nothing.
--dump-std: include standard-library functions in --dump.
--dump-filter TEXT: dump only functions whose name or symbol contains TEXT.
--fuzz: emit a coverage-instrumented libFuzzer executable that runs main once
  per input; main reads the input through fuzz_input_size/fuzz_input_byte.
--libfuzzer PATH: libFuzzer archive for --fuzz (default: clang's
  libclang_rt.fuzzer_no_main.a).
--shared-library / --static-library: emit C exports without requiring main.
--unloadable-library: check the Linux GNU x86_64 unloadable runtime subset.
  Requires --shared-library; foreign bindings must honor its teardown contract.
--runtime-interface: generate an explicit typed loader from checked export source.
--library-name NAME: namespace for C exports, header, and generated .plentyi.
--archiver PATH: ar-compatible archiver for --static-library (default: ar).
--extract-interface: read embedded .plentyi metadata without executing library code.
--verify-interface: check a generated interface against a binary before linking.
--emit-runtime: extract the matching runtime archive and native link arguments.
--version: print the compiler's version and target.
--linker PATH: cc-compatible linker driver (default: cc on PATH).
--link-arg ARG: pass one extra driver argument verbatim; may be repeated.
  Without either override, prefer installed mold, then lld, then cc's default.
  PLENTY_LINKER_THREADS=N limits that mold or lld to N threads.
--verbose: report the effective linker command.
--no-strip: keep the symbol table and debug sections in --compile and
  --shared-library output (stripped by default; --fuzz never strips).
--target TRIPLE: require this target to match the supported packaged runtime.
--fast-compile: skip optimization for faster compilation (optimized by default).
--release: compatibility alias for the optimized default.
--unchecked MODE: what plain `unchecked:` blocks outside std do with their
  arithmetic and indexing preconditions: `trap` (default) checks them and traps; `assume`
  omits the checks, so a violation is undefined behavior. `unchecked trap:`
  always traps.
--test-hooks: enable std.test failure injection for FILE or --compile.
  Intended for testing ordinary programs; omitted builds contain no hooks.
--no-cache: compile/link without cached functions, objects or executables
  (also PLENTY_CACHE=off; PLENTY_CACHE_DIR selects the directory).
--no-program-cache: reuse compiled functions but not program objects/executables.
--cache-stats: report the cache's hits and misses after compiling.
";

pub fn usage(program: &str) -> String {
    USAGE.replace("{0}", program)
}

/// Write `dump`'s files into the directory `output`, or to standard output
/// under `#### NAME` headers when `output` is `-`. Stage files a previous dump
/// left in the directory are removed, so none is mistaken for this one's.
fn write_dump(dump: &plenty::Dump, output: &str) -> Result<(), Box<dyn Error>> {
    use std::io::Write;
    if output == "-" {
        let mut stdout = std::io::stdout().lock();
        for (name, text) in &dump.files {
            writeln!(stdout, "#### {name}")?;
            stdout.write_all(text.as_bytes())?;
        }
        return Ok(());
    }
    let directory = Path::new(output);
    std::fs::create_dir_all(directory)?;
    for stage in plenty::DumpStage::ALL {
        let name = format!("{}.txt", stage.name());
        let path = directory.join(&name);
        if path.is_file() && !dump.files.iter().any(|(written, _)| *written == name) {
            std::fs::remove_file(path)?;
        }
    }
    for (name, text) in &dump.files {
        std::fs::write(directory.join(name), text)?;
    }
    Ok(())
}

fn distribution_filename(stem: &OsStr, suffix: &str) -> OsString {
    let mut name = stem.to_os_string();
    // Append rather than replace an extension: source stems can contain dots.
    name.push(suffix);
    name
}

/// Run the compiler interface on `args`, excluding the program name.
pub fn run(program: &str, mut args: Vec<String>) -> Result<ExitCode, Box<dyn Error>> {
    let mut options = plenty::CompileOptions::default();
    let mut library_name = None;
    let mut unloadable = false;
    let mut archiver = None;
    let mut libfuzzer = None;
    let mut dist = None;
    let mut link_options = false;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--unloadable-library" {
            if unloadable {
                return Err("--unloadable-library may be specified only once".into());
            }
            unloadable = true;
            args.remove(index);
            continue;
        }
        if matches!(
            args[index].as_str(),
            "--linker"
                | "--link-arg"
                | "--target"
                | "--library-name"
                | "--archiver"
                | "--libfuzzer"
                | "--dist"
        ) {
            let flag = args.remove(index);
            if index == args.len() {
                return Err(format!("{flag} requires a value").into());
            }
            let value = args.remove(index);
            if flag == "--dist" {
                if value.is_empty()
                    || value.starts_with('-')
                    || dist.replace(std::path::PathBuf::from(value)).is_some()
                {
                    return Err(
                        "--dist requires one nonempty directory, specified only once".into(),
                    );
                }
                continue;
            }
            if flag == "--library-name" {
                if library_name.replace(value).is_some() {
                    return Err("--library-name may be specified only once".into());
                }
                continue;
            }
            if flag == "--libfuzzer" {
                if value.is_empty() || libfuzzer.replace(std::path::PathBuf::from(value)).is_some()
                {
                    return Err("--libfuzzer requires one nonempty path".into());
                }
                continue;
            }
            if flag == "--archiver" {
                if value.is_empty() || archiver.replace(std::path::PathBuf::from(value)).is_some() {
                    return Err("--archiver requires one nonempty path".into());
                }
                continue;
            }
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
            options.auto_linker = false;
        } else {
            index += 1;
        }
    }
    if let Some(index) = args.iter().position(|a| a == "--verbose") {
        args.remove(index);
        options.verbose = true;
        link_options = true;
    }
    let root = if let Some(index) = args.iter().position(|a| a == "--module-root") {
        args.remove(index);
        if index == args.len() {
            return Err("--module-root requires a directory".into());
        }
        Some(std::path::PathBuf::from(args.remove(index)))
    } else {
        None
    };
    let graph = if let Some(index) = args.iter().position(|a| a == "--package-graph") {
        args.remove(index);
        if index == args.len() {
            return Err("--package-graph requires a file".into());
        }
        Some(std::path::PathBuf::from(args.remove(index)))
    } else {
        None
    };
    if graph.is_some() && root.is_some() {
        return Err("--package-graph excludes --module-root".into());
    }
    if let Some(index) = args.iter().position(|a| a == "--no-cache") {
        args.remove(index);
        options.cache = plenty::cache::CacheMode::Off;
    }
    if let Some(index) = args.iter().position(|a| a == "--no-program-cache") {
        args.remove(index);
        options.program_cache = false;
    }
    if let Some(index) = args.iter().position(|a| a == "--cache-stats") {
        args.remove(index);
        options.cache_stats = true;
    }
    if let Some(index) = args.iter().position(|a| a == "--test-hooks") {
        args.remove(index);
        options.test_hooks = true;
    }
    // Each check mode checks one program and then exits, so the program need
    // not be freed (issue #321).
    let mut check = plenty::CheckOptions {
        skip_teardown: true,
        ..plenty::CheckOptions::default()
    };
    if let Some(index) = args.iter().position(|a| a == "--check-all-methods") {
        args.remove(index);
        check.all_methods = true;
    }
    let mut dump = plenty::DumpOptions::default();
    let mut dump_options = false;
    if let Some(index) = args.iter().position(|a| a == "--dump-std") {
        args.remove(index);
        dump.standard_library = true;
        dump_options = true;
    }
    if let Some(index) = args.iter().position(|a| a == "--dump-filter") {
        args.remove(index);
        if index == args.len() || args[index].is_empty() {
            return Err("--dump-filter requires nonempty text".into());
        }
        dump.filter = Some(args.remove(index));
        dump_options = true;
    }
    let mut profile_flag = args
        .iter()
        .find(|a| a.as_str() == "--release" || a.as_str() == "--fast-compile")
        .cloned();
    if let Some(index) = args.iter().position(|a| a == "--unchecked") {
        args.remove(index);
        if index == args.len() {
            return Err("--unchecked requires `trap` or `assume`".into());
        }
        options.unchecked = plenty::UncheckedMode::parse(&args.remove(index))?;
        profile_flag.get_or_insert_with(|| "--unchecked".into());
    }
    if let Some(index) = args.iter().position(|a| a == "--fast-compile") {
        args.remove(index);
        options.optimize = false;
    }
    if let Some(index) = args.iter().position(|a| a == "--release") {
        args.remove(index);
    }
    if let Some(index) = args.iter().position(|a| a == "--no-strip") {
        args.remove(index);
        options.strip = false;
    }
    if args.iter().any(|a| a == "--tests") {
        // A single file has no `tests/` directory; tests belong to packages.
        return Err("--tests requires a package: run `plenty check --tests` in a directory with plenty.toml".into());
    }
    // Validate modes only after removing standalone options, so their order
    // cannot hide the selected mode (issue #590).
    if check.all_methods
        && !args
            .first()
            .is_some_and(|a| a == "--check" || a == "--check-module")
    {
        // Compiling would need the unused bodies' code or a separate cache
        // entry; checking is where a library author asks for this.
        return Err("--check-all-methods requires --check or --check-module".into());
    }
    if let Some(flag) = profile_flag {
        let compiles = match args.as_slice() {
            [source] => !source.starts_with('-'),
            [flag, ..] => {
                flag == "--compile"
                    || flag == "--fuzz"
                    || flag == "--dump"
                    || flag == "--emit-object"
                    || flag == "--shared-library"
                    || flag == "--static-library"
            }
            [] => false,
        };
        if !compiles {
            return Err(format!("{flag} requires a compilation mode").into());
        }
    }
    if !options.strip
        && !args
            .first()
            .is_some_and(|a| a == "--compile" || a == "--shared-library")
    {
        return Err("--no-strip requires --compile or --shared-library".into());
    }
    if dump_options && !args.first().is_some_and(|a| a == "--dump") {
        return Err("--dump-std and --dump-filter require --dump".into());
    }
    if options.test_hooks {
        let executable = match args.as_slice() {
            [source] => !source.starts_with('-'),
            [flag, ..] => flag == "--compile",
            [] => false,
        };
        if !executable {
            return Err("--test-hooks requires FILE or --compile".into());
        }
    }
    if options.target.is_some() {
        plenty::validate_target(options.target.as_deref())?;
    }
    let library = args
        .first()
        .is_some_and(|a| matches!(a.as_str(), "--shared-library" | "--static-library"));
    if unloadable && !args.first().is_some_and(|a| a == "--shared-library") {
        return Err("--unloadable-library requires --shared-library".into());
    }
    let extraction = args.first().is_some_and(|a| a == "--extract-interface");
    let runtime_interface = args.first().is_some_and(|a| a == "--runtime-interface");
    if library_name.is_some() && !library && !extraction && !runtime_interface {
        return Err(
            "--library-name requires library output or interface generation/extraction".into(),
        );
    }
    if libfuzzer.is_some() && !args.first().is_some_and(|a| a == "--fuzz") {
        return Err("--libfuzzer requires --fuzz".into());
    }
    if archiver.is_some() && !args.first().is_some_and(|a| a == "--static-library") {
        return Err("--archiver requires --static-library".into());
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
                        | "--dump"
                        | "--emit-runtime"
                        | "--extract-interface"
                        | "--verify-interface"
                        | "--runtime-interface"
                        | "-V"
                        | "--version"
                        | "--print-target"
                )
            }))
    {
        return Err("linker options require compilation or execution".into());
    }
    if let Some(destination) = dist {
        if !matches!(args.as_slice(), [flag, source] if flag == "--compile" && !source.starts_with('-'))
        {
            return Err("--dist requires --compile FILE without -o/--output".into());
        }
        if !options.link_args.is_empty() {
            return Err(
                "--dist excludes --link-arg; bundled executables must use only $ORIGIN RUNPATH"
                    .into(),
            );
        }
        let source = Path::new(&args[1]);
        let stem = source
            .file_stem()
            .filter(|stem| !stem.is_empty())
            .ok_or("--dist requires a source filename with a nonempty stem")?;
        let output = destination.join(distribution_filename(stem, std::env::consts::EXE_SUFFIX));
        options.distribution = true;
        if let Some(graph) = graph {
            let graph = plenty::project::SourceGraph::read(&graph)?;
            plenty::compile_package_to_executable(source, &graph, &output, &options)?;
        } else {
            compile(source, &output, root.as_deref(), &options)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if let Some(graph) = graph {
        return with_graph(program, &graph, &args, &options, &check);
    }
    match args.as_slice() {
        [flag, source, option, output]
            if flag == "--runtime-interface" && (option == "-o" || option == "--output") =>
        {
            let name =
                library_name.ok_or("runtime interface generation requires --library-name NAME")?;
            plenty::emit_runtime_interface(
                Path::new(source),
                Path::new(output),
                root.as_deref(),
                &name,
            )?;
        }
        [flag, binary, interface] if flag == "--verify-interface" && root.is_none() => {
            plenty::verify_library_interface(Path::new(binary), Path::new(interface))?;
        }
        [flag, source, option, output]
            if flag == "--extract-interface"
                && root.is_none()
                && (option == "-o" || option == "--output") =>
        {
            let name = library_name.ok_or("interface extraction requires --library-name NAME")?;
            plenty::extract_library_interface(Path::new(source), &name, Path::new(output))?;
        }
        [flag, source, option, output] if library && (option == "-o" || option == "--output") => {
            let name = library_name.ok_or("library output requires --library-name NAME")?;
            let kind = if flag == "--static-library" {
                plenty::LibraryKind::Static
            } else {
                plenty::LibraryKind::Shared
            };
            let mut library = plenty::LibraryOptions::new(name, kind);
            library.compile = options;
            library.unloadable = unloadable;
            if let Some(archiver) = archiver {
                library.archiver = archiver;
            }
            plenty::compile_file_to_library(
                Path::new(source),
                Path::new(output),
                root.as_deref(),
                &library,
            )?;
        }
        [flag]
            if matches!(flag.as_str(), "-V" | "--version") && root.is_none() && !link_options =>
        {
            println!(
                "{program} {} ({})",
                env!("CARGO_PKG_VERSION"),
                plenty::native_target()
            );
        }
        [flag] if flag == "--print-target" && root.is_none() && !link_options => {
            println!(
                "{} (CPU baseline: {})",
                plenty::native_target(),
                plenty::native_cpu_baseline()
            );
        }
        [flag, directory] if flag == "--emit-runtime" && root.is_none() => {
            plenty::emit_runtime(Path::new(directory))?;
        }
        [flag, source, option, output]
            if flag == "--emit-object" && (option == "-o" || option == "--output") =>
        {
            plenty::compile_file_to_object_with_options(
                Path::new(source),
                Path::new(output),
                root.as_deref(),
                &options,
            )?;
        }
        [flag, stages, source, option, output]
            if flag == "--dump" && (option == "-o" || option == "--output") =>
        {
            dump.stages = plenty::DumpStage::parse_list(stages)?;
            let result = plenty::dump_file(Path::new(source), root.as_deref(), &options, &dump)?;
            write_dump(&result, output)?;
        }
        [] => {
            print!("{}", usage(program));
        }
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{}", usage(program));
        }
        [flag, source] if (flag == "--check" || flag == "--check-module") => {
            let warnings = plenty::check_file_with_options(
                Path::new(source),
                root.as_deref(),
                flag == "--check",
                &check,
            )?;
            print_warnings(&warnings);
        }
        [flag, source, option, output]
            if flag == "--fuzz" && (option == "-o" || option == "--output") =>
        {
            let mut options = options;
            options.fuzz = Some(match libfuzzer {
                Some(path) => path,
                None => plenty::find_libfuzzer()?,
            });
            compile(
                Path::new(source),
                Path::new(output),
                root.as_deref(),
                &options,
            )?;
        }
        [flag, source, option, output]
            if flag == "--compile" && (option == "-o" || option == "--output") =>
        {
            compile(
                Path::new(source),
                Path::new(output),
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
                root.as_deref(),
                &plenty::CompileOptions {
                    executable_cache: true,
                    temporary_output: true,
                    ..options.clone()
                },
            )?;
            // Inherit stdin, stdout, stderr, environment, and working directory.
            // Keep the temporary directory alive until the child has exited.
            let status = Command::new(&executable).status()?;
            return Ok(exit_code(status));
        }
        _ => return Err(format!("unrecognised arguments\n{}", usage(program)).into()),
    }
    Ok(ExitCode::SUCCESS)
}

/// Lint warnings go to stderr and leave the exit status unchanged; see
/// book/src/design/toolchain/01-building/warnings.md.
fn print_warnings(warnings: &[plenty::Warning]) {
    for warning in warnings {
        eprintln!("{warning}");
    }
}

/// The modes that accept `--package-graph`. The graph's first package must
/// contain FILE.
fn with_graph(
    program: &str,
    graph: &Path,
    args: &[String],
    options: &plenty::CompileOptions,
    check: &plenty::CheckOptions,
) -> Result<ExitCode, Box<dyn Error>> {
    let graph = plenty::project::SourceGraph::read(graph)?;
    match args {
        [flag, source] if flag == "--check" || flag == "--check-module" => {
            let warnings =
                plenty::project::check_entry(Path::new(source), &graph, flag == "--check", check)?;
            print_warnings(&warnings);
        }
        [flag, source, option, output]
            if flag == "--compile" && (option == "-o" || option == "--output") =>
        {
            plenty::compile_package_to_executable(
                Path::new(source),
                &graph,
                Path::new(output),
                options,
            )?;
        }
        [source] if !source.starts_with('-') => {
            let workspace = tempfile::tempdir()?;
            let executable = workspace
                .path()
                .join(format!("program{}", std::env::consts::EXE_SUFFIX));
            plenty::compile_package_to_executable(
                Path::new(source),
                &graph,
                &executable,
                &plenty::CompileOptions {
                    executable_cache: true,
                    temporary_output: true,
                    ..options.clone()
                },
            )?;
            let status = Command::new(&executable).status()?;
            return Ok(exit_code(status));
        }
        _ => {
            return Err(format!(
                "--package-graph requires FILE, --check, --check-module, or --compile\n{}",
                usage(program)
            )
            .into());
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn compile(
    source: &Path,
    output: &Path,
    root: Option<&Path>,
    options: &plenty::CompileOptions,
) -> Result<(), Box<dyn Error>> {
    plenty::compile_file_to_executable_with_options(source, output, root, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_names_preserve_stems_and_append_platform_suffix() {
        for stem in ["analysis", "analysis.v2", "analysis file λ", "analysis.exe"] {
            for suffix in ["", ".exe"] {
                assert_eq!(
                    distribution_filename(OsStr::new(stem), suffix),
                    OsString::from(format!("{stem}{suffix}"))
                );
            }
        }
    }
}

//! Native toolchain configuration, independent of language lowering.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Target of the compiler and its embedded runtime. Native code generation
/// currently supports x86_64-unknown-linux-gnu only.
pub fn native_target() -> &'static str {
    env!("PLENTY_RUNTIME_TARGET")
}

/// Reject unsupported or mismatched targets before native lowering or linking.
/// Selecting a linker does not make cross-compilation possible.
pub fn validate_target(requested: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    validate_target_pair(requested.unwrap_or(native_target()), native_target())
}

fn validate_target_pair(requested: &str, packaged: &str) -> Result<(), Box<dyn std::error::Error>> {
    if requested != "x86_64-unknown-linux-gnu" {
        return Err(format!("unsupported native target `{requested}`; supported target: x86_64-unknown-linux-gnu (64-bit little-endian System V)").into());
    }
    if requested != packaged {
        return Err(format!("target `{requested}` does not match packaged runtime `{packaged}`; cross-compilation is not supported").into());
    }
    Ok(())
}

/// libFuzzer's archive without its `main`, as `clang` reports it: the
/// default `--libfuzzer` of `--fuzz` builds.
pub fn find_libfuzzer() -> Result<PathBuf, Box<dyn std::error::Error>> {
    const ARCHIVE: &str = "libclang_rt.fuzzer_no_main.a";
    let missing = || {
        format!(
            "libFuzzer not found: `clang -print-file-name={ARCHIVE}` names no file; install clang's compiler-rt or pass --libfuzzer PATH"
        )
    };
    let output = Command::new("clang")
        .arg(format!("-print-file-name={ARCHIVE}"))
        .output()
        .map_err(|error| format!("{}: {error}", missing()))?;
    // clang echoes the bare name when it has no such file.
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    if !output.status.success() || !path.is_absolute() || !path.is_file() {
        return Err(missing().into());
    }
    Ok(path)
}

/// The exact runtime embedded in this compiler and its native dependencies.
#[derive(Debug)]
pub struct RuntimeArtifacts {
    pub archive: PathBuf,
    pub link_args: Vec<OsString>,
}

/// Extract the packaged runtime for external linking. The directory is created
/// if necessary; the archive and `link-args.txt` (one argument per line) are
/// replaced. Use the runtime from the same compiler build as the object.
pub fn emit_runtime(directory: &Path) -> Result<RuntimeArtifacts, Box<dyn std::error::Error>> {
    validate_target(None)?;
    std::fs::create_dir_all(directory)?;
    let archive = directory.join("libplenty_runtime.a");
    let args: Vec<_> = crate::codegen::RUNTIME_LINK_ARGS
        .split_whitespace()
        .collect();
    std::fs::write(&archive, crate::codegen::RUNTIME_ARCHIVE)?;
    std::fs::write(
        directory.join("target.txt"),
        format!("{}\n", native_target()),
    )?;
    std::fs::write(
        directory.join("link-args.txt"),
        format!("{}\n", args.join("\n")),
    )?;
    Ok(RuntimeArtifacts {
        archive,
        link_args: args.into_iter().map(Into::into).collect(),
    })
}

/// Options for native compilation. Linkers must accept the Unix C compiler
/// driver interface (object/archive inputs, native `-l` flags, and `-o OUT`).
/// Use a wrapper for drivers with another interface; this is not a raw `ld` API.
#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// Optional explicit target; it must match the supported packaged runtime.
    pub target: Option<String>,
    /// Executable name or path, passed directly to Command without a shell.
    pub linker: PathBuf,
    /// Prefer an installed mold or lld for the default `cc` driver. Any
    /// explicit link arguments or a different driver disable this selection.
    pub auto_linker: bool,
    /// Report the effective driver command before linking.
    pub verbose: bool,
    /// Additional driver arguments, each passed verbatim after the inputs.
    pub link_args: Vec<OsString>,
    /// Override the optional dataframe engine location. Consulted only when
    /// reachable native code uses std.dataframe; otherwise no engine is needed.
    pub dataframe_engine: Option<PathBuf>,
    /// Optional faer engine, consulted only for reachable std.linalg calls.
    pub linalg_engine: Option<PathBuf>,
    /// Ask Cranelift to optimize generated code for speed (the release
    /// profile). Off by default, which favors compilation speed.
    pub optimize: bool,
    /// Where compiled functions are cached between compilations.
    pub cache: crate::cache::CacheMode,
    /// Reuse a whole program's object from the cache when none of its
    /// sources changed. Off, a build reuses only compiled functions, so it
    /// still measures the frontend and code generation.
    pub program_cache: bool,
    /// Print the cache's hit and miss counts after compiling.
    pub cache_stats: bool,
    /// Build a libFuzzer target linked with this libFuzzer archive (clang's
    /// `libclang_rt.fuzzer_no_main.a`): count every block's runs and enter
    /// through the fuzz driver. See book/src/design/toolchain/01-building/fuzzing.md.
    pub fuzz: Option<PathBuf>,
    /// Link executables and shared libraries with `-Wl,--strip-all`, dropping
    /// the symbol table and the debug sections the Rust runtime brings. No
    /// run-time path reads them; `.dynsym` and trap tables stay. `plenty test`
    /// and `--fuzz` builds keep them regardless. See
    /// book/src/design/toolchain/01-building/linking.md.
    pub strip: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            target: None,
            linker: "cc".into(),
            auto_linker: true,
            verbose: false,
            link_args: Vec::new(),
            dataframe_engine: None,
            linalg_engine: None,
            optimize: false,
            cache: crate::cache::CacheMode::Environment,
            program_cache: true,
            cache_stats: false,
            fuzz: None,
            strip: true,
        }
    }
}

impl CompileOptions {
    pub(crate) fn automatic_linker(&self) -> Option<&'static str> {
        if !cfg!(target_os = "linux")
            || !self.auto_linker
            || self.linker != Path::new("cc")
            || !self.link_args.is_empty()
        {
            return None;
        }
        static LINKER: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
        *LINKER.get_or_init(|| detect_linker(self.cache.directory().as_deref()))
    }
}

/// Automatic linker candidates, in order of preference.
const LINKERS: [&str; 2] = ["mold", "lld"];
/// The directory below the cache directory holding probe results (issue
/// #410), one file per toolchain state, named by its key.
const PROBE_DIRECTORY: &str = "linker-probe";
const PROBE_HEADER: &str = "plenty linker probe 1";
/// Records kept, newest first. Tests with fake toolchains add records to the
/// shared cache directory; an evicted record costs one probe.
const PROBE_RECORDS: usize = 64;

/// Choose the automatic linker, reusing a probe recorded in the cache
/// directory `cache` by an earlier process for the same toolchain. Without a
/// usable cache directory every process probes once.
fn detect_linker(cache: Option<&Path>) -> Option<&'static str> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let installed: Vec<(&'static str, PathBuf)> = LINKERS
        .into_iter()
        .filter_map(|name| find_on_path(&path, &format!("ld.{name}")).map(|found| (name, found)))
        .collect();
    if installed.is_empty() {
        return None;
    }
    let record = cache.and_then(|dir| {
        let key = probe_key(
            &path,
            std::env::var_os("COMPILER_PATH").as_deref(),
            &installed,
        )?;
        Some((dir.join(PROBE_DIRECTORY), key))
    });
    if let Some((directory, key)) = &record
        && let Some(found) = read_probe(directory, key)
    {
        return found;
    }
    // A name on PATH alone is insufficient: the driver might not support
    // -fuse-ld, or its tool search may find a broken linker.
    let found = installed.iter().map(|(name, _)| *name).find(|name| {
        Command::new("cc")
            .arg(format!("-fuse-ld={name}"))
            .args(["-Wl,--version", "-o", "/dev/null"])
            .output()
            .is_ok_and(|output| output.status.success())
    });
    if let Some((directory, key)) = &record {
        // Failing to record the result only costs the next process a probe.
        let _ = write_probe(directory, key, found);
    }
    found
}

/// What the probe's outcome depends on: the search paths, and the identity
/// (resolved path, size, modification time) of `cc` and of each candidate
/// linker found on `PATH`. Installing, removing or upgrading any of them
/// changes the key. `None` when `cc` is not on `PATH`.
fn probe_key(
    path: &std::ffi::OsStr,
    compiler_path: Option<&std::ffi::OsStr>,
    installed: &[(&str, PathBuf)],
) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    field(PROBE_HEADER.as_bytes());
    field(path.as_encoded_bytes());
    field(compiler_path.unwrap_or_default().as_encoded_bytes());
    let cc = find_on_path(path, "cc")?;
    for (name, file) in std::iter::once(("cc", &cc)).chain(installed.iter().map(|(n, f)| (*n, f))) {
        let resolved = std::fs::canonicalize(file).ok()?;
        let metadata = std::fs::metadata(&resolved).ok()?;
        let modified = metadata
            .modified()
            .ok()?
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .ok()?;
        field(name.as_bytes());
        field(file.as_os_str().as_encoded_bytes());
        field(resolved.as_os_str().as_encoded_bytes());
        field(&metadata.len().to_le_bytes());
        field(&modified.as_nanos().to_le_bytes());
    }
    Some(format!("{:x}", hash.finalize()))
}

/// The choice recorded for `key`: `Some(None)` records that no candidate
/// works. `None` when nothing is recorded.
fn read_probe(directory: &Path, key: &str) -> Option<Option<&'static str>> {
    let text = std::fs::read_to_string(directory.join(key)).ok()?;
    let mut lines = text.lines();
    if lines.next()? != PROBE_HEADER {
        return None;
    }
    match lines.next()? {
        "none" => Some(None),
        chosen => LINKERS.into_iter().find(|name| *name == chosen).map(Some),
    }
}

/// Written to a temporary name and renamed, like cache entries, so a
/// concurrent reader sees a whole record or none. Then the oldest records
/// beyond [`PROBE_RECORDS`] are removed.
fn write_probe(directory: &Path, key: &str, found: Option<&str>) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".{key}.{}.tmp", std::process::id()));
    std::fs::write(
        &temporary,
        format!("{PROBE_HEADER}\n{}\n", found.unwrap_or("none")),
    )?;
    std::fs::rename(&temporary, directory.join(key)).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })?;
    let mut records: Vec<_> = std::fs::read_dir(directory)?
        .flatten()
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    if records.len() > PROBE_RECORDS {
        records.sort_by_key(|record| std::cmp::Reverse(record.0));
        for (_, path) in &records[PROBE_RECORDS..] {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

/// The first executable `name` on `path`, as command lookup finds it.
fn find_on_path(path: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| executable(candidate))
}

/// Driver arguments limiting the automatic linker's threads, from
/// `PLENTY_LINKER_THREADS=N` (issue #415). Parallel test runs link many
/// programs at once, and each multithreaded link then competes with the
/// others for the cores; one thread per link is faster overall there. Unset
/// or empty keeps the linker's default.
fn thread_args(linker: &str, value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let count: std::num::NonZeroU32 = value
        .parse()
        .map_err(|_| format!("PLENTY_LINKER_THREADS must be a positive integer, not `{value}`"))?;
    Ok(match linker {
        "mold" => Some(format!("-Wl,--thread-count={count}")),
        "lld" => Some(format!("-Wl,--threads={count}")),
        _ => None,
    })
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    path.is_file()
}

#[cfg_attr(feature = "profile", hotpath::measure)]
pub(crate) fn link(
    object: &Path,
    runtime: &Path,
    native_args: &str,
    output: &Path,
    options: &CompileOptions,
    extra_args: &[OsString],
    trailing_args: &[OsString],
) -> Result<(), Box<dyn std::error::Error>> {
    let engines = engine_link_args(object, options)?;
    let mut command = Command::new(&options.linker);
    if let Some(linker) = options.automatic_linker() {
        command.arg(format!("-fuse-ld={linker}"));
        let threads = std::env::var("PLENTY_LINKER_THREADS").ok();
        if let Some(arg) = thread_args(linker, threads.as_deref())? {
            command.arg(arg);
        }
    }
    command
        .arg(object)
        .arg(runtime)
        .args(extra_args)
        .args(&options.link_args)
        .args(trailing_args)
        .args(engines)
        .args(native_args.split_whitespace());
    // Last before `-o`, so wrapper drivers still see the inputs first.
    if options.strip {
        command.arg("-Wl,--strip-all");
    }
    command.arg("-o").arg(output);
    if options.verbose {
        eprintln!("link: {command:?}");
    }
    let result = command.output().map_err(|error| {
        // Release users have no Rust toolchain, so name what to install.
        let hint = if error.kind() == std::io::ErrorKind::NotFound {
            "; Plenty links programs with a C compiler driver: install gcc or clang \
                 and the C library development files, or select a cc-compatible driver \
                 with --linker"
        } else {
            "; select a cc-compatible driver with --linker"
        };
        format!(
            "failed to invoke linker driver `{}`: {error}{hint}",
            options.linker.display()
        )
    })?;
    if !result.status.success() {
        return Err(format!(
            "linker driver `{}` failed ({}); command: {command:?}\n{}{}",
            options.linker.display(),
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}

/// A large native library that ships outside the runtime archive, behind a
/// trusted `std` facade, and is linked only when reached. See
/// book/src/design/toolchain/01-building/engines.md for the contract.
pub(crate) struct Engine {
    /// The facade module, a trusted `.plentyi` interface in `stdlib/`.
    pub(crate) module: &'static str,
    /// File name and SONAME: `libplenty_NAME_vN.so`, N the C ABI generation.
    library: &'static str,
    /// Every facade symbol starts with this, and no other symbol does.
    symbol_prefix: &'static str,
    /// Compile-time override of the engine path.
    environment: &'static str,
    /// The `CompileOptions` override, which takes precedence.
    option: fn(&CompileOptions) -> Option<&PathBuf>,
    /// What the engine is, then how to get it, for the missing-engine error.
    description: &'static str,
    install: &'static str,
}

pub(crate) const ENGINES: &[Engine] = &[
    Engine {
        module: "std.dataframe",
        library: "libplenty_dataframe_v1.so",
        symbol_prefix: "pldf_v1_",
        environment: "PLENTY_DATAFRAME_ENGINE",
        option: |options| options.dataframe_engine.as_ref(),
        description: "the optional Polars engine",
        install: "Build it with python3 scripts/build_dataframe_engine.py --out COMPILER_DIRECTORY",
    },
    Engine {
        module: "std.linalg",
        library: "libplenty_linalg_v1.so",
        symbol_prefix: "plla_v1_",
        environment: "PLENTY_LINALG_ENGINE",
        option: |options| options.linalg_engine.as_ref(),
        description: "the optional faer linear algebra engine",
        install: "Build it with python3 scripts/build_native_engine.py linalg --out COMPILER_DIRECTORY",
    },
];

impl Engine {
    /// The explicit option, then the environment, then beside the compiler.
    pub(crate) fn selected(&self, options: &CompileOptions) -> std::io::Result<PathBuf> {
        if let Some(path) = (self.option)(options)
            .cloned()
            .or_else(|| std::env::var_os(self.environment).map(PathBuf::from))
        {
            return Ok(path);
        }
        Ok(std::env::current_exe()?.with_file_name(self.library))
    }
}

/// The engines reached from the object's roots. Follow section relocations,
/// as --gc-sections does: looking for undefined symbols alone would count
/// unused local helpers and destructors, requiring an engine even for a bare
/// unused import.
fn reached_engines<'a>(
    bytes: &[u8],
    engines: &'a [Engine],
) -> Result<Vec<&'a Engine>, Box<dyn std::error::Error>> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget, SectionFlags};
    let object = object::File::parse(bytes)?;
    let engine_of = |name: &str| {
        engines
            .iter()
            .position(|e| name.starts_with(e.symbol_prefix))
    };
    if !object.symbols().any(|symbol| {
        symbol.is_undefined() && symbol.name().is_ok_and(|name| engine_of(name).is_some())
    }) {
        return Ok(Vec::new());
    }
    let mut pending: Vec<_> = object
        .symbols()
        .filter(|symbol| symbol.is_global() && !symbol.is_undefined())
        .filter_map(|symbol| symbol.section_index())
        .collect();
    // Exported library interfaces are explicitly retained even without a
    // global symbol. Their relocations are roots too.
    pending.extend(
        object
            .sections()
            .filter(|section| {
                matches!(
                    section.flags(), SectionFlags::Elf { sh_flags, .. }
                    if sh_flags.0 & object::elf::SHF_GNU_RETAIN.0 != 0
                )
            })
            .map(|section| section.index()),
    );
    let mut reached = vec![false; engines.len()];
    let mut seen = std::collections::HashSet::new();
    while let Some(index) = pending.pop() {
        if !seen.insert(index) {
            continue;
        }
        for (_, relocation) in object.section_by_index(index)?.relocations() {
            match relocation.target() {
                RelocationTarget::Symbol(index) => {
                    let symbol = object.symbol_by_index(index)?;
                    if symbol.is_undefined() {
                        if let Some(engine) = engine_of(symbol.name()?) {
                            reached[engine] = true;
                        }
                    } else if let Some(section) = symbol.section_index() {
                        pending.push(section);
                    }
                }
                RelocationTarget::Section(section) => pending.push(section),
                _ => {}
            }
        }
    }
    Ok(engines
        .iter()
        .zip(reached)
        .filter(|(_, r)| *r)
        .map(|(e, _)| e)
        .collect())
}

/// The reached engines' libraries, then a RUNPATH of the executable's own
/// directory followed by each engine's directory.
fn engine_link_args(
    object: &Path,
    options: &CompileOptions,
) -> Result<Vec<OsString>, Box<dyn std::error::Error>> {
    let reached = reached_engines(&std::fs::read(object)?, ENGINES)?;
    if reached.is_empty() {
        return Ok(Vec::new());
    }
    let mut libraries = Vec::new();
    let mut directories: Vec<OsString> = vec!["$ORIGIN".into()];
    for engine in reached {
        let selected = engine.selected(options)?;
        let path = selected.canonicalize().map_err(|error| {
            format!(
                "{} requires {}; cannot open {}: {error}. {}, or set {} to {}",
                engine.module,
                engine.description,
                selected.display(),
                engine.install,
                engine.environment,
                engine.library
            )
        })?;
        if !path.is_file() {
            return Err(
                format!("{} engine is not a file: {}", engine.module, path.display()).into(),
            );
        }
        let directory = path.parent().unwrap().as_os_str().to_owned();
        // ELF RUNPATH is colon-separated even when passed as one driver argument.
        if directory.as_encoded_bytes().contains(&b':') {
            return Err(format!(
                "{} engine directory must not contain ':' (ELF RUNPATH separator)",
                engine.module
            )
            .into());
        }
        if !directories.contains(&directory) {
            directories.push(directory);
        }
        libraries.push(path.into_os_string());
    }
    for directory in directories {
        libraries.extend([
            "-Xlinker".into(),
            "-rpath".into(),
            "-Xlinker".into(),
            directory,
        ]);
    }
    Ok(libraries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn linker_candidates_must_be_executable_files() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        assert!(!executable(temp.path()));
        let candidate = temp.path().join("ld.mold");
        assert!(!executable(&candidate));
        std::fs::write(&candidate, "fixture").unwrap();
        std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!executable(&candidate));
        std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(executable(&candidate));
    }

    #[cfg(unix)]
    #[test]
    fn probe_key_changes_with_the_toolchain() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let tool = |name: &str| {
            let file = temp.path().join(name);
            std::fs::write(&file, name).unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
            file
        };
        let (cc, mold) = (tool("cc"), tool("ld.mold"));
        let path = temp.path().as_os_str();
        let installed = [("mold", mold.clone())];
        let key = probe_key(path, None, &installed).unwrap();
        assert_eq!(probe_key(path, None, &installed).as_ref(), Some(&key));
        // The linker is no longer found.
        assert_ne!(probe_key(path, None, &[]).as_ref(), Some(&key));
        assert_ne!(
            probe_key(path, Some("/usr/libexec/mold".as_ref()), &installed).as_ref(),
            Some(&key)
        );
        // `cc` is replaced or touched.
        let touch = |file: &Path| {
            let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(when)
                .unwrap();
        };
        touch(&cc);
        let touched = probe_key(path, None, &installed).unwrap();
        assert_ne!(touched, key);
        touch(&mold);
        assert_ne!(probe_key(path, None, &installed).unwrap(), touched);
        std::fs::remove_file(&cc).unwrap();
        assert_eq!(probe_key(path, None, &installed), None);
    }

    #[test]
    fn probe_records_are_reused_only_for_their_key() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("cache").join(PROBE_DIRECTORY);
        assert_eq!(read_probe(&directory, "a"), None);
        write_probe(&directory, "a", Some("mold")).unwrap();
        assert_eq!(read_probe(&directory, "a"), Some(Some("mold")));
        // A stale key misses, so the caller probes again.
        assert_eq!(read_probe(&directory, "b"), None);
        write_probe(&directory, "b", None).unwrap();
        assert_eq!(read_probe(&directory, "b"), Some(None));
        assert_eq!(read_probe(&directory, "a"), Some(Some("mold")));
        std::fs::write(directory.join("b"), format!("{PROBE_HEADER}\nbfd\n")).unwrap();
        assert_eq!(read_probe(&directory, "b"), None);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
    }

    #[test]
    fn probe_records_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join(PROBE_DIRECTORY);
        for i in 0..PROBE_RECORDS + 5 {
            let key = format!("key{i}");
            write_probe(&directory, &key, Some("lld")).unwrap();
            // Distinct ages, in write order, whatever the clock's resolution.
            let when = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000_000 + i as u64);
            std::fs::File::options()
                .write(true)
                .open(directory.join(&key))
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            PROBE_RECORDS
        );
        assert_eq!(read_probe(&directory, "key4"), None);
        assert_eq!(read_probe(&directory, "key5"), Some(Some("lld")));
    }

    #[test]
    fn linker_threads_apply_to_mold_and_lld() {
        assert_eq!(thread_args("mold", None), Ok(None));
        assert_eq!(thread_args("mold", Some(" ")), Ok(None));
        assert_eq!(
            thread_args("mold", Some("1")),
            Ok(Some("-Wl,--thread-count=1".into()))
        );
        assert_eq!(
            thread_args("lld", Some("4")),
            Ok(Some("-Wl,--threads=4".into()))
        );
        assert!(thread_args("mold", Some("0")).is_err());
        assert!(thread_args("mold", Some("many")).is_err());
    }

    #[test]
    fn rejects_runtime_mismatch_and_unsupported_layouts() {
        let native = "x86_64-unknown-linux-gnu";
        assert!(validate_target_pair(native, native).is_ok());
        assert!(
            validate_target_pair(native, "aarch64-unknown-linux-gnu")
                .unwrap_err()
                .to_string()
                .contains("does not match")
        );
        for target in [
            "i686-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "not-a-target",
            "x86_64-unknown-linux-musl",
        ] {
            assert!(
                validate_target_pair(target, target)
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported native target")
            );
        }
    }

    fn fake_engine(module: &'static str, symbol_prefix: &'static str) -> Engine {
        Engine {
            module,
            library: "libplenty_fake_v1.so",
            symbol_prefix,
            environment: "PLENTY_FAKE_ENGINE",
            option: |_| None,
            description: "a fake engine",
            install: "",
        }
    }

    /// A second engine needs only a table entry: reachability is per engine,
    /// and an unused facade function does not require its engine.
    #[test]
    fn only_reached_engines_are_linked() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("native.plentyi"),
            "extern def used() -> i64 = \"plfa_v1_used\"\n\
             extern def unused() -> i64 = \"plfb_v1_unused\"\n\
             pub def first() -> i64:\n    used()\n\
             pub def second() -> i64:\n    unused()\n",
        )
        .unwrap();
        let source = temp.path().join("main.plenty");
        std::fs::write(
            &source,
            "import native\ndef main() -> ():\n    x = native.first()\n",
        )
        .unwrap();
        let object = temp.path().join("main.o");
        crate::compile_file_to_object(&source, &object, None).unwrap();
        let engines = [
            fake_engine("std.a", "plfa_v1_"),
            fake_engine("std.b", "plfb_v1_"),
        ];
        let bytes = std::fs::read(&object).unwrap();
        let reached: Vec<_> = reached_engines(&bytes, &engines)
            .unwrap()
            .into_iter()
            .map(|engine| engine.module)
            .collect();
        assert_eq!(reached, ["std.a"]);
        assert!(reached_engines(&bytes, ENGINES).unwrap().is_empty());
    }

    #[test]
    fn engine_prefixes_and_libraries_are_distinct() {
        for (i, a) in ENGINES.iter().enumerate() {
            assert!(a.library.starts_with("libplenty_") && a.library.ends_with(".so"));
            for b in &ENGINES[i + 1..] {
                assert!(!a.symbol_prefix.starts_with(b.symbol_prefix));
                assert!(!b.symbol_prefix.starts_with(a.symbol_prefix));
                assert_ne!(a.library, b.library);
                assert_ne!(a.environment, b.environment);
            }
        }
    }
}

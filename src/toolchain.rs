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
        format!("libFuzzer not found: `clang -print-file-name={ARCHIVE}` names no file; install clang's compiler-rt or pass --libfuzzer PATH")
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
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            target: None,
            linker: "cc".into(),
            auto_linker: true,
            verbose: false,
            link_args: Vec::new(),
            optimize: false,
            cache: crate::cache::CacheMode::Environment,
            program_cache: true,
            cache_stats: false,
            fuzz: None,
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
        *LINKER.get_or_init(|| {
            let path = std::env::var_os("PATH").unwrap_or_default();
            ["mold", "lld"].into_iter().find(|name| {
                let installed = std::env::split_paths(&path)
                    .any(|dir| executable(&dir.join(format!("ld.{name}"))));
                // A name on PATH alone is insufficient: the driver might not
                // support -fuse-ld, or its tool search may find a broken linker.
                installed
                    && Command::new("cc")
                        .arg(format!("-fuse-ld={name}"))
                        .args(["-Wl,--version", "-o", "/dev/null"])
                        .output()
                        .is_ok_and(|output| output.status.success())
            })
        })
    }
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
    let mut command = Command::new(&options.linker);
    if let Some(linker) = options.automatic_linker() {
        command.arg(format!("-fuse-ld={linker}"));
    }
    command
        .arg(object)
        .arg(runtime)
        .args(extra_args)
        .args(&options.link_args)
        .args(trailing_args)
        .args(native_args.split_whitespace())
        .arg("-o")
        .arg(output);
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

    #[test]
    fn rejects_runtime_mismatch_and_unsupported_layouts() {
        let native = "x86_64-unknown-linux-gnu";
        assert!(validate_target_pair(native, native).is_ok());
        assert!(validate_target_pair(native, "aarch64-unknown-linux-gnu")
            .unwrap_err()
            .to_string()
            .contains("does not match"));
        for target in [
            "i686-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "not-a-target",
            "x86_64-unknown-linux-musl",
        ] {
            assert!(validate_target_pair(target, target)
                .unwrap_err()
                .to_string()
                .contains("unsupported native target"));
        }
    }
}

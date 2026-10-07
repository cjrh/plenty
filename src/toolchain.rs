//! Native toolchain configuration, independent of language lowering.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Options for native compilation. Linkers must accept the Unix C compiler
/// driver interface (object/archive inputs, native `-l` flags, and `-o OUT`).
/// Use a wrapper for drivers with another interface; this is not a raw `ld` API.
#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// Executable name or path, passed directly to Command without a shell.
    pub linker: PathBuf,
    /// Additional driver arguments, each passed verbatim after the inputs.
    pub link_args: Vec<OsString>,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            linker: "cc".into(),
            link_args: Vec::new(),
        }
    }
}

pub(crate) fn link(
    object: &Path,
    runtime: &Path,
    native_args: &str,
    output: &Path,
    options: &CompileOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = Command::new(&options.linker)
        .arg(object)
        .arg(runtime)
        .args(&options.link_args)
        .args(native_args.split_whitespace())
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|error| {
            format!(
                "failed to invoke linker driver `{}`: {error}; select a cc-compatible driver with --linker",
                options.linker.display()
            )
        })?;
    if !result.status.success() {
        return Err(format!(
            "linker driver `{}` failed ({}):\n{}{}",
            options.linker.display(),
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(())
}

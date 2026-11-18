//! A closed set of source-toolchain components; packages cannot supply a
//! Cargo manifest, build command, header, or expected ownership contract.
use super::{CompileOptions, RuntimeInput, native_target};
use crate::frontend::Inputs;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    Iced,
    Egui,
}

impl Backend {
    pub(crate) fn for_module(module: &str) -> Option<Self> {
        match module {
            "std.gui.retained.iced" => Some(Self::Iced),
            "std.gui.immediate.egui" => Some(Self::Egui),
            _ => None,
        }
    }

    pub(crate) fn selected(inputs: &Inputs) -> Result<Option<Self>> {
        let mut selected = None;
        for backend in inputs.standard_imports().filter_map(Self::for_module) {
            if selected.is_some_and(|previous| previous != backend) {
                return Err("mixing std.gui.retained.iced and std.gui.immediate.egui in one executable is unsupported".into());
            }
            selected = Some(backend);
        }
        Ok(selected)
    }

    pub(crate) fn exports(self) -> &'static str {
        match self {
            Self::Iced => "iced_app",
            Self::Egui => "egui_app",
        }
    }

    pub(crate) fn link(
        self,
        object: &Path,
        output: &Path,
        options: &CompileOptions,
        inputs: &Inputs,
    ) -> Result<()> {
        if native_target() != "x86_64-unknown-linux-gnu"
            || options.distribution
            || options.test_hooks
            || options.cpu != crate::CpuLevel::Baseline
            || options.fuzz.is_some()
        {
            return Err(
                "std.gui requires an ordinary baseline Linux GNU x86-64 package executable".into(),
            );
        }
        let (name, include_var, guard) = match self {
            Self::Iced => (
                "iced",
                "PLENTY_ICED_APP_DIR",
                include_str!("../../experiments/packages/iced/native/expected-contract.txt").trim(),
            ),
            Self::Egui => (
                "egui",
                "PLENTY_EGUI_APP_DIR",
                include_str!("../../experiments/packages/egui/native/expected-contract.txt").trim(),
            ),
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target = root.join("target");
        if !std::env::current_exe()?.starts_with(&target) {
            return Err("std.gui requires the source-built compiler under its matching checkout's target/; binary-only installs have no GUI support sources".into());
        }
        let sources: Vec<_> = inputs.sources().map(Path::to_owned).collect();
        crate::library::publication::validate(&[output], &sources)?;
        let native = root.join("experiments/packages").join(name).join("native");
        // Never discover Cargo configuration in the consumer package. Use a
        // reviewed *fixed* header, not C text generated from user exports.
        let inputs = Command::new("python3")
            .arg("-I")
            .arg(root.join("scripts/link_native_executable.py"))
            .arg("--print-inputs")
            .arg("--manifest-path")
            .arg(native.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(target.join("std-gui").join(name))
            .arg("--lib-name")
            .arg(format!("plenty_{name}"))
            .args(["--features", "application"])
            .env(include_var, native.join("reviewed"))
            .env("CARGO_NET_OFFLINE", "true")
            .current_dir("/")
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| {
                format!("building std.gui {name} support (requires Python 3 and Cargo): {e}")
            })?;
        if !inputs.status.success() {
            return Err(format!(
                "building locked std.gui {name} support failed ({})",
                inputs.status
            )
            .into());
        }
        let (archive, args): (PathBuf, Vec<String>) = serde_json::from_slice(&inputs.stdout)?;
        let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        args.extend([
            "-Wl,--gc-sections".into(),
            format!("-Wl,--require-defined={guard}").into(),
        ]);
        // The archive includes the callback shims and therefore roots their
        // exports. Do not restrict engine scanning to Plenty main's calls.
        let options = CompileOptions {
            // lld does not implement --require-defined. Prefer the normal cc
            // driver over an automatic lld; never weaken the contract check.
            auto_linker: options.auto_linker && options.automatic_linker() != Some("lld"),
            executable_exports: None,
            executable_cache: false,
            ..options.clone()
        };
        let directory = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staged = tempfile::tempdir_in(directory)?;
        let built = staged.path().join("application");
        super::link(
            object,
            RuntimeInput::Existing(&archive),
            "",
            &built,
            &options,
            &[],
            &args,
        )?;
        crate::library::publication::validate(&[output], &sources)?;
        std::fs::rename(built, output)?;
        Ok(())
    }
}

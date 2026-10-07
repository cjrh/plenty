//! Native C library packaging. Source visibility remains independent of exports.
use crate::{exports::Interface, CompileOptions};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
pub(crate) mod publication;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const RUNTIME: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/libplenty_library_runtime.a"));
const NATIVE_ARGS: &str = include_str!(concat!(env!("OUT_DIR"), "/library-runtime-link-args.txt"));

/// Generate a typed runtime-loading module from the same checked export source
/// used to build a library. Does not emit, link, load, or execute native code.
pub fn runtime_interface_source(path: &Path, root: Option<&Path>, name: &str) -> Result<String> {
    Ok(runtime_interface(path, root, name)?.0)
}

fn runtime_interface(
    path: &Path,
    root: Option<&Path>,
    name: &str,
) -> Result<(String, Vec<PathBuf>)> {
    crate::validate_target(None)?;
    let mut heap = crate::value::Heap::default();
    let program = crate::frontend::compile_file(path, root, false, &mut heap)?;
    crate::op::check(&program.ops)?;
    let interface = Interface::new(name, &program.exports)?;
    interface.validate_imports(&program.imported_symbols)?;
    let source = interface.runtime_source(&program.exports)?;
    crate::frontend::check_generated_interface(&source)?;
    Ok((source, program.source_paths))
}

/// Publish a generated runtime interface without overwriting any loaded source.
pub fn emit_runtime_interface(
    path: &Path,
    output: &Path,
    root: Option<&Path>,
    name: &str,
) -> Result<()> {
    if output
        .extension()
        .is_none_or(|extension| extension != "plentyi")
    {
        return Err("runtime interface output must have the .plentyi extension".into());
    }
    let (source, paths) = runtime_interface(path, root, name)?;
    publication::validate(&[output], &paths)?;
    let directory = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let workspace = tempfile::Builder::new()
        .prefix(".plenty-build-")
        .tempdir_in(directory)?;
    let staged = workspace.path().join("interface");
    std::fs::write(&staged, source)?;
    publication::validate(&[output], &paths)?;
    publication::publish(workspace, &[(staged, output.to_owned())])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryKind {
    Static,
    Shared,
}

#[derive(Clone, Debug)]
pub struct LibraryOptions {
    /// Importable ASCII name; C symbols must start with this name and `_`.
    pub name: String,
    pub kind: LibraryKind,
    pub compile: CompileOptions,
    /// An ar-compatible program, used only for static library output.
    pub archiver: PathBuf,
}

impl LibraryOptions {
    pub fn new(name: impl Into<String>, kind: LibraryKind) -> Self {
        Self {
            name: name.into(),
            kind,
            compile: CompileOptions::default(),
            archiver: "ar".into(),
        }
    }
}

/// Companion files are written beside the library, using its explicit name.
#[derive(Debug)]
pub struct LibraryArtifacts {
    pub header: PathBuf,
    pub interface: PathBuf,
    /// Native dependencies, one driver argument per line, for static consumers.
    pub link_args: PathBuf,
}

pub fn compile_file_to_library(
    path: &Path,
    output: &Path,
    root: Option<&Path>,
    options: &LibraryOptions,
) -> Result<LibraryArtifacts> {
    crate::validate_target(options.compile.target.as_deref())?;
    crate::exports::check_library_name(&options.name)?;
    let mut heap = crate::value::Heap::default();
    let program = crate::frontend::compile_file(path, root, false, &mut heap)?;
    crate::op::check(&program.ops)?;
    let interface = Interface::new(&options.name, &program.exports)?;
    interface.validate_imports(&program.imported_symbols)?;
    let directory = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let artifacts = LibraryArtifacts {
        header: directory.join(format!("{}.h", options.name)),
        interface: directory.join(format!("{}.plentyi", options.name)),
        link_args: directory.join(format!("{}.link-args.txt", options.name)),
    };
    let destinations = [
        artifacts.header.as_path(),
        artifacts.interface.as_path(),
        artifacts.link_args.as_path(),
        output,
    ];
    publication::validate(&destinations, &program.source_paths)?;
    if [
        &artifacts.header,
        &artifacts.interface,
        &artifacts.link_args,
    ]
    .iter()
    .any(|p| p.file_name() == output.file_name())
    {
        return Err("library output conflicts with a generated companion file".into());
    }
    let workspace = tempfile::Builder::new()
        .prefix(".plenty-build-")
        .tempdir_in(directory)?;
    let object = workspace.path().join("plenty_exports.o");
    crate::codegen::emit_library_object(&program, &heap, &object, &interface)?;
    let built = workspace.path().join("library");
    match options.kind {
        LibraryKind::Static => {
            // Add the entry object to the runtime archive; the archive index lets
            // the native linker extract only required members, without a main.
            std::fs::write(&built, RUNTIME)?;
            let result = Command::new(&options.archiver)
                .arg("rcs")
                .arg(&built)
                .arg(&object)
                .output()
                .map_err(|e| {
                    format!(
                        "failed to invoke archiver `{}`: {e}",
                        options.archiver.display()
                    )
                })?;
            if !result.status.success() {
                return Err(format!(
                    "archiver failed ({}): {}",
                    result.status,
                    String::from_utf8_lossy(&result.stderr)
                )
                .into());
            }
        }
        LibraryKind::Shared => {
            let runtime = workspace.path().join("runtime.a");
            std::fs::write(&runtime, RUNTIME)?;
            let script = workspace.path().join("exports.map");
            let symbols = program
                .exports
                .iter()
                .map(|e| e.symbol.as_str())
                .chain(interface.handles.iter().map(|h| h.destroy.as_str()))
                .chain(std::iter::once(interface.discovery.as_str()))
                .chain(std::iter::once(interface.contract_guard.as_str()))
                .collect::<Vec<_>>();
            std::fs::write(
                &script,
                format!("{{ global: {}; local: *; }};\n", symbols.join("; ")),
            )?;
            let mut compile = options.compile.clone();
            compile.link_args.extend([
                OsString::from("-shared"),
                OsString::from("-Wl,-z,defs"),
                OsString::from("-Xlinker"),
                OsString::from("--version-script"),
                OsString::from("-Xlinker"),
                script.into_os_string(),
            ]);
            crate::toolchain::link(&object, &runtime, NATIVE_ARGS, &built, &compile)?;
        }
    }
    // Publish only after compilation/linking succeeds; a diagnostic leaves old outputs alone.
    let args = if options.kind == LibraryKind::Static {
        options
            .compile
            .link_args
            .iter()
            .map(|a| a.to_str().ok_or("static link arguments must be UTF-8"))
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .chain(NATIVE_ARGS.split_whitespace())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if args.iter().any(|a| a.contains(['\n', '\r'])) {
        return Err("static link arguments must not contain newlines".into());
    }
    let header = workspace.path().join("header");
    let source = workspace.path().join("interface");
    let arguments = workspace.path().join("link-args");
    std::fs::write(&header, interface.header)?;
    std::fs::write(&source, interface.source)?;
    std::fs::write(
        &arguments,
        args.iter().map(|a| format!("{a}\n")).collect::<String>(),
    )?;
    publication::validate(&destinations, &program.source_paths)?;
    publication::publish(
        workspace,
        &[
            (header, artifacts.header.clone()),
            (source, artifacts.interface.clone()),
            (arguments, artifacts.link_args.clone()),
            (built, output.to_owned()),
        ],
    )?;
    Ok(artifacts)
}

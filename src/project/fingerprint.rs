//! Build reuse: a target is rebuilt unless a digest of everything that can
//! change its output equals the one stored by its last successful build, and
//! the files that build read are still current.
//!
//! The digest covers the name of every regular file and symbolic link (with
//! its target's name) below each package's source root, so an added module
//! rebuilds even if it shadows nothing; the package graph, the entry, the
//! kind of target, the code generation and linker options, and the
//! compiler's identity. The compiler embeds `std` and the runtime, so its
//! identity covers both.
//!
//! The contents are not hashed here. The stored fingerprint holds the
//! frontend's [`Inputs`](crate::frontend::Inputs): every source and included
//! file the build read, with its digest. A target is fresh only while they
//! are [current](crate::frontend::Inputs::current), the check the program
//! cache makes, which also reruns each include's boundary and ownership
//! checks. Includes may read any file in the package directory, which also
//! holds build outputs, `.pkg/` and perhaps version control data, so that
//! directory is never walked.
use super::SourceGraph;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn digest(
    graph: &SourceGraph,
    entry: &Path,
    kind: &str,
    options: &crate::CompileOptions,
) -> Result<String> {
    let mut hash = Sha256::new();
    let mut field = |text: &str| {
        hash.update((text.len() as u64).to_le_bytes());
        hash.update(text.as_bytes());
    };
    field(env!("CARGO_PKG_VERSION"));
    field(crate::cache::compiler_identity());
    field(kind);
    field(&entry.display().to_string());
    // Moving/replacing an optional shared engine changes the RUNPATH of an
    // executable that reaches it. Missing/unused engines never fail fingerprinting.
    for engine in crate::toolchain::ENGINES {
        let path = engine.selected(options).ok();
        field(&format!("{path:?}"));
        if let Some(path) = path {
            field(&format!(
                "{:?}",
                std::fs::metadata(path)
                    .ok()
                    .map(|m| (m.len(), m.modified().ok()))
            ));
        }
    }
    field(&format!(
        "{:?} {:?} {:?} {} {} {} {} {:?}",
        options.target,
        options.linker,
        options.link_args,
        options.optimize,
        options.unchecked.name(),
        options.strip,
        options.test_hooks,
        // Package libraries are checked, not linked.
        if kind == "library" {
            None
        } else {
            options.automatic_linker()
        }
    ));
    for package in &graph.packages {
        field(&package.identity);
        field(&package.root.display().to_string());
        field(&format!(
            "{:?} {:?} {:?}",
            package.directory, package.library, package.dependencies
        ));
        let mut files = vec![];
        walk(&package.root, &mut files)?;
        files.sort();
        for file in files {
            field(&file.display().to_string());
            let error = |e: std::io::Error| format!("reading {}: {e}", file.display());
            if file
                .symlink_metadata()
                .map_err(error)?
                .file_type()
                .is_symlink()
            {
                field(&format!(
                    "link {:?}",
                    std::fs::read_link(&file).map_err(error)?
                ));
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// The regular files and symbolic links below `directory`. Links are not
/// followed, so a loop or a link to `/` or a device costs one entry.
fn walk(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    let error = |e: std::io::Error| format!("reading {}: {e}", directory.display());
    for entry in std::fs::read_dir(directory).map_err(error)? {
        let entry = entry.map_err(error)?;
        let kind = entry.file_type().map_err(error)?;
        if kind.is_dir() {
            walk(&entry.path(), files)?;
        } else if kind.is_file() || kind.is_symlink() {
            files.push(entry.path());
        }
    }
    Ok(())
}

/// One target's stored digest, under `target/PROFILE/.fingerprint/`,
/// followed by a newline and the encoded inputs of its last build.
pub(super) struct Fingerprint {
    path: PathBuf,
    digest: String,
}

impl Fingerprint {
    pub(super) fn new(directory: &Path, name: &str, digest: String) -> Fingerprint {
        Fingerprint {
            path: directory.join(".fingerprint").join(name),
            digest,
        }
    }

    /// Whether the last successful build had these inputs, its output (if
    /// any) still exists, and a build of `graph` would read the same files.
    pub(super) fn fresh(&self, output: Option<&Path>, graph: &SourceGraph) -> bool {
        let Ok(stored) = std::fs::read(&self.path) else {
            return false;
        };
        let Some(newline) = stored.iter().position(|&byte| byte == b'\n') else {
            return false;
        };
        let (digest, inputs) = (&stored[..newline], &stored[newline + 1..]);
        output.is_none_or(Path::is_file)
            && digest == self.digest.as_bytes()
            && crate::frontend::Inputs::decode(inputs).is_some_and(|inputs| inputs.current(graph))
    }

    /// Forget the stored digest before building, so an interrupted build
    /// cannot leave a partial output that a later digest would vouch for.
    pub(super) fn invalidate(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Store the digest and `inputs`. Inputs with a path that is not UTF-8
    /// cannot be encoded, so such a build is not recorded and always reruns.
    /// The file is written beside its final name and renamed, so a reader
    /// sees the old fingerprint, none, or the whole new one.
    pub(super) fn record(&self, inputs: &crate::frontend::Inputs) -> Result<()> {
        let Some(inputs) = inputs.encode() else {
            return Ok(());
        };
        let mut bytes = self.digest.clone().into_bytes();
        bytes.push(b'\n');
        bytes.extend_from_slice(&inputs);
        std::fs::create_dir_all(self.path.parent().unwrap())?;
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(format!(".{}.tmp", std::process::id()));
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

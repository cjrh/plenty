//! Build reuse: a target is rebuilt only when a digest of everything that can
//! change its output differs from the one stored by its last successful build.
//!
//! The digest covers every file below each package's source root (not only
//! the modules a build happened to read, so an added module that shadows
//! nothing still counts), the package graph, the entry, the kind of target,
//! the code generation and linker options, and the compiler's identity. The
//! compiler embeds `std` and the runtime, so its identity covers both.
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
        "{:?} {:?} {:?} {} {} {} {:?}",
        options.target,
        options.linker,
        options.link_args,
        options.optimize,
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
        field(&format!("{:?} {:?}", package.library, package.dependencies));
        let mut files = vec![];
        walk(&package.root, &mut files)?;
        files.sort();
        for file in files {
            field(&file.display().to_string());
            let bytes =
                std::fs::read(&file).map_err(|e| format!("reading {}: {e}", file.display()))?;
            field(&format!("{:x}", Sha256::digest(&bytes)));
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn walk(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// One target's stored digest, under `target/PROFILE/.fingerprint/`.
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

    /// Whether the last successful build had these inputs and its output
    /// (if any) still exists.
    pub(super) fn fresh(&self, output: Option<&Path>) -> bool {
        output.is_none_or(Path::is_file)
            && std::fs::read_to_string(&self.path).is_ok_and(|stored| stored == self.digest)
    }

    /// Forget the stored digest before building, so an interrupted build
    /// cannot leave a partial output that a later digest would vouch for.
    pub(super) fn invalidate(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    pub(super) fn record(&self) -> Result<()> {
        std::fs::create_dir_all(self.path.parent().unwrap())?;
        std::fs::write(&self.path, &self.digest)?;
        Ok(())
    }
}

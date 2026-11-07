//! Build reuse: a target is rebuilt only when a digest of everything that can
//! change its output differs from the one stored by its last successful build.
//!
//! The digest covers every regular file and symbolic link (its target's
//! name, not its contents) below each package's source root and `assets/`
//! directory (not only the modules and included files a build
//! happened to read, so an added module that shadows nothing still counts),
//! the package graph, the entry, the kind of target,
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
    // A package and its `tests/` package share `assets/`; hash it once.
    let mut walked = std::collections::HashSet::new();
    for package in &graph.packages {
        field(&package.identity);
        field(&package.root.display().to_string());
        field(&format!(
            "{:?} {:?} {:?} {:?}",
            package.directory, package.assets, package.library, package.dependencies
        ));
        let mut files = vec![];
        walk(&package.root, &mut files)?;
        if let Some(assets) = &package.assets
            && super::real_assets(assets)?
            && walked.insert(assets)
        {
            walk(assets, &mut files)?;
        }
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
                // The target is either inside a walked directory, and hashed
                // there, or outside, where neither imports nor includes may
                // read it.
                field(&format!(
                    "link {:?}",
                    std::fs::read_link(&file).map_err(error)?
                ));
            } else {
                let bytes = std::fs::read(&file).map_err(error)?;
                field(&format!("{:x}", Sha256::digest(&bytes)));
            }
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// The regular files and symbolic links below `directory`. Links are not
/// followed, so a loop or a link to `/` or a device costs one entry. The
/// caller rejects a top-level `assets` link before walking it.
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

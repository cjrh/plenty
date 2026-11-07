//! Content-validated cache for the embedded runtime's Rust dependencies.
//!
//! Native compilation stays with the native toolchain. Runtime archive keys
//! include its finished artifacts as well as runtime source/configuration.
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct Fingerprint(Sha256);

impl Fingerprint {
    pub fn new() -> Self {
        Self(Sha256::new())
    }

    pub fn field(&mut self, bytes: impl AsRef<[u8]>) {
        let bytes = bytes.as_ref();
        self.0.update((bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
    }

    /// Include names as well as contents so additions, removals and renames
    /// invalidate the entry, including files restored with old mtimes by CI.
    pub fn tree(&mut self, root: &Path) -> io::Result<()> {
        self.tree_at(root, root)
    }

    fn tree_at(&mut self, root: &Path, path: &Path) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("cache inputs must not contain symlinks: {}", path.display()),
            ));
        }
        self.field(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        );
        if metadata.is_dir() {
            self.field(b"directory");
            let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                self.tree_at(root, &entry.path())?;
            }
        } else {
            self.field(b"file");
            self.field(fs::read(path)?);
        }
        Ok(())
    }

    pub fn finish(self) -> String {
        format!("{:x}", self.0.finalize())
    }
}

/// Native archives are rebuilt or restored by the native toolchain first. Hash
/// their actual contents rather than guessing their SDK/header dependencies.
pub struct ArchiveInputs<'a> {
    pub recipe: &'a str,
    pub source: &'a Path,
    pub manifest: &'a Path,
    pub vendor: &'a Path,
    pub target_os: &'a str,
    pub application: bool,
    pub checked: bool,
}

impl ArchiveInputs<'_> {
    pub fn key(&self) -> io::Result<String> {
        let mut key = Fingerprint::new();
        key.field("plenty-runtime-archive-v1");
        key.field(self.recipe);
        key.tree(self.source)?;
        key.tree(self.manifest)?;
        key.field(self.target_os);
        key.field([u8::from(self.application), u8::from(self.checked)]);
        let mut artifacts = fs::read_dir(self.vendor)?.collect::<Result<Vec<_>, _>>()?;
        artifacts.sort_by_key(|entry| entry.file_name());
        for artifact in artifacts {
            let path = artifact.path();
            if path
                .extension()
                .is_some_and(|ext| ext == "a" || ext == "rlib" || ext == "lib")
            {
                key.field(artifact.file_name().as_encoded_bytes());
                key.tree(&path)?;
            }
        }
        Ok(key.finish())
    }
}

fn manifest(dir: &Path, files: &[String]) -> io::Result<String> {
    let mut text = String::new();
    for file in files {
        let bytes = fs::read(dir.join(file))?;
        if bytes.is_empty() {
            return Err(io::Error::other("empty cached runtime dependency"));
        }
        text.push_str(&format!("{:x} {file}\n", Sha256::digest(bytes)));
    }
    Ok(text)
}

fn valid(dir: &Path, files: &[String]) -> bool {
    match (
        fs::read_to_string(dir.join("complete")),
        manifest(dir, files),
    ) {
        (Ok(saved), Ok(actual)) => saved == actual,
        _ => false,
    }
}

/// Publish only a complete set of validated artifacts. Concurrent builds may
/// populate the same key; a per-key lock serializes publication. A failed build
/// leaves no cache entry, and a truncated/restored entry is rebuilt.
pub fn get_or_build(
    root: &Path,
    key: &str,
    files: &[String],
    build: impl FnOnce(&Path),
) -> io::Result<(PathBuf, bool)> {
    fs::create_dir_all(root)?;
    // Keep the lock file in place: unlinking it could let a waiter lock a
    // different inode. Serialization also protects replacement of a corrupt
    // entry from deleting another builder's freshly published directory.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(format!("{key}.lock")))?;
    lock.lock()?;
    let entry = root.join(key);
    if valid(&entry, files) {
        return Ok((entry, true));
    }
    let staging = tempfile::Builder::new()
        .prefix("pending-")
        .tempdir_in(root)?;
    build(staging.path());
    fs::write(
        staging.path().join("complete"),
        manifest(staging.path(), files)?,
    )?;
    if entry.exists() && !valid(&entry, files) {
        fs::remove_dir_all(&entry)?;
    }
    match fs::rename(staging.path(), &entry) {
        Ok(()) => Ok((entry, false)),
        Err(_) if valid(&entry, files) => Ok((entry, true)),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn key(path: &Path) -> String {
        let mut key = Fingerprint::new();
        key.tree(path).unwrap();
        key.finish()
    }

    #[test]
    fn content_key_tracks_names_additions_removals_and_same_length_edits() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.rs");
        fs::write(&input, "first").unwrap();
        let first = key(dir.path());
        let modified = fs::metadata(&input).unwrap().modified().unwrap();
        fs::write(&input, "other").unwrap();
        fs::File::options()
            .write(true)
            .open(&input)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_ne!(first, key(dir.path()));
        fs::write(&input, "first").unwrap();
        assert_eq!(first, key(dir.path()));
        fs::rename(&input, dir.path().join("renamed.rs")).unwrap();
        assert_ne!(first, key(dir.path()));
        fs::rename(dir.path().join("renamed.rs"), &input).unwrap();
        fs::write(dir.path().join("added.rs"), "added").unwrap();
        assert_ne!(first, key(dir.path()));
        fs::remove_file(dir.path().join("added.rs")).unwrap();
        assert_eq!(first, key(dir.path()));
    }

    #[test]
    fn runtime_archives_track_sources_native_outputs_and_both_modes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src");
        let vendor = dir.path().join("vendor");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&vendor).unwrap();
        fs::write(source.join("lib.rs"), "source").unwrap();
        let manifest = dir.path().join("Cargo.toml");
        fs::write(&manifest, "manifest").unwrap();
        fs::write(vendor.join("libnative.a"), "native").unwrap();
        fs::write(vendor.join("librust.rlib"), "rust").unwrap();
        fs::write(vendor.join("native.lib"), "MSVC native").unwrap();
        let mut inputs = ArchiveInputs {
            recipe: "toolchain/recipe",
            source: &source,
            manifest: &manifest,
            vendor: &vendor,
            target_os: "linux",
            application: true,
            checked: false,
        };
        let original = inputs.key().unwrap();
        inputs.checked = true;
        let checked = inputs.key().unwrap();
        inputs.application = false;
        let library = inputs.key().unwrap();
        inputs.checked = false;
        let unchecked_library = inputs.key().unwrap();
        let mut modes = [original.clone(), checked, library, unchecked_library];
        modes.sort();
        assert!(modes.windows(2).all(|pair| pair[0] != pair[1]));
        inputs.application = true;
        for file in [
            source.join("lib.rs"),
            manifest.clone(),
            vendor.join("libnative.a"),
            vendor.join("librust.rlib"),
            vendor.join("native.lib"),
        ] {
            let previous = fs::read(&file).unwrap();
            fs::write(&file, "changed").unwrap();
            assert_ne!(original, inputs.key().unwrap());
            fs::write(&file, previous).unwrap();
            assert_eq!(original, inputs.key().unwrap());
        }
        inputs.recipe = "different compiler/flags/target/recipe";
        assert_ne!(original, inputs.key().unwrap());
        inputs.recipe = "toolchain/recipe";
        fs::write(vendor.join("temporary.o"), "not linked into runtime").unwrap();
        assert_eq!(original, inputs.key().unwrap());
        fs::write(vendor.join("additional.a"), "new linked archive").unwrap();
        assert_ne!(original, inputs.key().unwrap());
    }

    #[test]
    fn fields_are_length_delimited_and_track_configuration() {
        let mut a = Fingerprint::new();
        a.field("ab");
        a.field("c");
        let mut b = Fingerprint::new();
        b.field("a");
        b.field("bc");
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn complete_entry_is_reused_but_changed_or_missing_artifacts_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec!["libone.rlib".to_owned(), "libtwo.rlib".to_owned()];
        let build = |out: &Path| {
            for file in &files {
                fs::write(out.join(file), file).unwrap();
            }
        };
        let (entry, hit) = get_or_build(dir.path(), "key", &files, build).unwrap();
        assert!(!hit);
        assert!(
            get_or_build(dir.path(), "key", &files, |_| panic!("cache miss"))
                .unwrap()
                .1
        );
        fs::write(entry.join(&files[0]), "corrupt").unwrap();
        assert!(!get_or_build(dir.path(), "key", &files, build).unwrap().1);
        fs::remove_file(entry.join(&files[1])).unwrap();
        assert!(!get_or_build(dir.path(), "key", &files, build).unwrap().1);
    }

    #[test]
    fn failed_build_is_not_published() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec!["libone.rlib".to_owned()];
        let result = std::panic::catch_unwind(|| {
            get_or_build(dir.path(), "key", &files, |out| {
                fs::write(out.join(&files[0]), "partial").unwrap();
                panic!("compiler failed");
            })
        });
        assert!(result.is_err());
        assert!(!dir.path().join("key").exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    fn concurrent_builds(corrupt: bool) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let files = vec!["libone.rlib".to_owned()];
        if corrupt {
            fs::create_dir(dir.path().join("key")).unwrap();
            fs::write(dir.path().join("key/complete"), "invalid manifest").unwrap();
        }
        let barrier = Arc::new(Barrier::new(2));
        let builds = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let barrier = Arc::clone(&barrier);
                let root = dir.path();
                let files = &files;
                let builds = &builds;
                scope.spawn(move || {
                    barrier.wait();
                    let (entry, _) = get_or_build(root, "key", files, |out| {
                        builds.fetch_add(1, Ordering::SeqCst);
                        fs::write(out.join(&files[0]), "complete artifact").unwrap();
                    })
                    .unwrap();
                    assert!(valid(&entry, files));
                });
            }
        });
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn concurrent_builds_publish_one_complete_entry() {
        concurrent_builds(false);
    }

    #[test]
    fn concurrent_builds_replace_one_corrupt_entry() {
        concurrent_builds(true);
    }
}

//! Places an engine library beside an ordinary (non-`--dist`) Windows or
//! macOS executable, where the platform loader finds it.
//!
//! Engines are large (the Polars engine is about 150 MB), so an unchanged
//! engine is not rewritten. A changed one is staged under a fresh name in the
//! destination directory and renamed over the old file: macOS caches code
//! signatures per vnode, so a loaded or signed dylib must get a new inode, not
//! be truncated in place. Copying to a path that does not exist yet also lets
//! macOS clone the file on APFS instead of copying its bytes.
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

/// `temporary` marks an output removed after a single run, where a hard link
/// is safe and avoids copying. Persistent outputs always get their own file,
/// so they never share an inode with the compiler's engine.
pub(super) fn install(source: &Path, destination: &Path, temporary: bool) -> std::io::Result<()> {
    if same_contents(source, destination)? {
        return Ok(());
    }
    if temporary && std::fs::hard_link(source, destination).is_ok() {
        return Ok(());
    }
    let directory = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::Builder::new()
        .prefix(".plenty-engine-")
        .tempdir_in(directory)?;
    let staged = staging
        .path()
        .join(destination.file_name().unwrap_or_default());
    std::fs::copy(source, &staged)?;
    std::fs::rename(&staged, destination)
}

fn same_contents(source: &Path, destination: &Path) -> std::io::Result<bool> {
    let mut existing = match File::open(destination) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut selected = File::open(source)?;
    if !existing.metadata()?.is_file() || existing.metadata()?.len() != selected.metadata()?.len() {
        return Ok(false);
    }
    let mut left = vec![0; 1 << 20];
    let mut right = vec![0; 1 << 20];
    loop {
        let read = selected.read(&mut left)?;
        if read == 0 {
            return Ok(true);
        }
        existing.read_exact(&mut right[..read])?;
        if left[..read] != right[..read] {
            return Ok(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &str = "libplenty_dataframe_v1.dylib";

    #[cfg(unix)]
    fn inode(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).unwrap().ino()
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_old_file_handles_and_hard_links() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("selected.dylib");
        let destination = root.path().join(LIBRARY);
        std::fs::write(&source, b"first signed engine").unwrap();
        install(&source, &destination, false).unwrap();
        assert_ne!(inode(&destination), inode(&source));
        let mut previous = File::open(&destination).unwrap();
        let old = inode(&destination);
        let link = root.path().join("running-engine.dylib");
        std::fs::hard_link(&destination, &link).unwrap();
        std::fs::write(&source, b"replacement signed engine").unwrap();
        install(&source, &destination, false).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"replacement signed engine"
        );
        assert_ne!(inode(&destination), old);
        let mut bytes = Vec::new();
        previous.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"first signed engine");
        assert_eq!(std::fs::read(&link).unwrap(), b"first signed engine");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_engine_is_not_rewritten() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("selected.dylib");
        let destination = root.path().join(LIBRARY);
        std::fs::write(&source, b"signed engine").unwrap();
        install(&source, &destination, false).unwrap();
        let first = inode(&destination);
        install(&source, &destination, false).unwrap();
        assert_eq!(inode(&destination), first);
        // Same length, different bytes: replaced.
        std::fs::write(&source, b"Signed engine").unwrap();
        install(&source, &destination, false).unwrap();
        assert_ne!(inode(&destination), first);
        assert_eq!(std::fs::read(&destination).unwrap(), b"Signed engine");
    }

    #[cfg(unix)]
    #[test]
    fn temporary_output_links_instead_of_copying() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("selected.dylib");
        let destination = root.path().join(LIBRARY);
        std::fs::write(&source, b"signed engine").unwrap();
        install(&source, &destination, true).unwrap();
        assert_eq!(inode(&destination), inode(&source));
        // An existing different file is replaced, never written through.
        let other = root.path().join("other.dylib");
        std::fs::write(&other, b"other engine").unwrap();
        install(&other, &destination, true).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"signed engine");
        assert_eq!(std::fs::read(&destination).unwrap(), b"other engine");
    }

    #[test]
    fn failed_copy_or_publication_preserves_existing_output() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("selected.dylib");
        let destination = root.path().join(LIBRARY);
        std::fs::write(&destination, b"old signed engine").unwrap();
        assert!(install(&source, &destination, false).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"old signed engine");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        std::fs::write(&source, b"new signed engine").unwrap();
        let directory = root.path().join("occupied");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("keep"), b"existing output").unwrap();
        assert!(install(&source, &directory, false).is_err());
        assert_eq!(
            std::fs::read(directory.join("keep")).unwrap(),
            b"existing output"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    }
}

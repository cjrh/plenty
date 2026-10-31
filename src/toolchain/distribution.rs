//! Staged application distributions. Never publish an incomplete directory or
//! overwrite an existing destination, including one created during linking.
use super::{ENGINES, Engine, native_target};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) struct Bundle {
    stage: tempfile::TempDir,
    destination: PathBuf,
}

impl Bundle {
    pub(super) fn new(output: &Path) -> Result<Self> {
        let name = output
            .file_name()
            .ok_or("distribution needs an executable filename")?;
        if ENGINES
            .iter()
            .any(|e| name == e.library || name == e.notices)
        {
            return Err(
                "distribution executable name conflicts with an engine or notices directory".into(),
            );
        }
        let destination = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or("distribution needs a destination directory")?;
        match std::fs::symlink_metadata(destination) {
            Ok(_) => {
                return Err(format!(
                    "distribution destination already exists: {}; choose a new directory",
                    destination.display()
                )
                .into());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let stage = tempfile::Builder::new()
            .prefix(".plenty-dist-")
            .tempdir_in(parent)?;
        Ok(Self {
            stage,
            destination: destination.to_path_buf(),
        })
    }

    pub(super) fn path(&self) -> &Path {
        self.stage.path()
    }

    pub(super) fn executable(&self, output: &Path) -> PathBuf {
        self.path().join(output.file_name().unwrap())
    }

    pub(super) fn publish(self) -> Result<()> {
        // Refuse replacement in the rename itself, including an empty directory
        // created after staging began. std::fs::rename allows replacement on
        // Windows too (its fallback uses POSIX rename semantics).
        #[cfg(windows)]
        let publication = rename_directory_noreplace(self.path(), &self.destination);
        #[cfg(not(windows))]
        let publication = rustix::fs::renameat_with(
            rustix::fs::CWD,
            self.path(),
            rustix::fs::CWD,
            &self.destination,
            rustix::fs::RenameFlags::NOREPLACE,
        );
        publication.map_err(|e| {
            format!(
                "cannot publish distribution {} (destination must not exist): {e}",
                self.destination.display()
            )
        })?;
        // The directory now has its final name; do not clean up the old name.
        let _ = self.stage.keep();
        Ok(())
    }
}

/// MoveFileExW with no replacement flag fails atomically if any destination
/// already exists. Unlike std::fs::rename, do not retry with replacement flags.
#[cfg(windows)]
fn rename_directory_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    // Canonicalize existing directories to obtain absolute, verbatim Windows
    // paths. This supports long paths without requiring a process manifest.
    let source = std::fs::canonicalize(source)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = destination.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination needs a filename",
        )
    })?;
    let destination = std::fs::canonicalize(parent)?.join(name);
    let wide = |path: &Path| -> std::io::Result<Vec<u16>> {
        let mut path: Vec<u16> = path.as_os_str().encode_wide().collect();
        if path.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains a NUL character",
            ));
        }
        path.push(0);
        Ok(path)
    };
    let source = wide(&source)?;
    let destination = wide(&destination)?;
    // SAFETY: Both pointers refer to live, NUL-terminated UTF-16 strings. The
    // API retains neither pointer; flags=0 forbids replacement and cross-volume
    // copying, so publication remains a single no-replace rename.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|e| format!("reading artifact {}: {e}", source.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "expected a regular distribution artifact: {}",
            source.display()
        )
        .into());
    }
    std::fs::copy(source, destination)
        .map_err(|e| format!("copying artifact {}: {e}", source.display()))?;
    Ok(())
}

fn copy_licenses(source: &Path, destination: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|e| format!("reading license directory {}: {e}", source.display()))?;
    if !metadata.file_type().is_dir() {
        return Err(format!("expected a license directory: {}", source.display()).into());
    }
    std::fs::create_dir(destination)?;
    let mut count = 0;
    for entry in std::fs::read_dir(source)
        .map_err(|e| format!("reading license directory {}: {e}", source.display()))?
    {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_licenses(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
        count += 1;
    }
    if count == 0 {
        return Err(format!("empty license directory: {}", source.display()).into());
    }
    Ok(())
}

pub(super) fn copy_engine(engine: &Engine, selected: &Path, bundle: &Path) -> Result<PathBuf> {
    copy_engine_inner(engine, selected, bundle).map_err(|error| {
        format!("cannot bundle {} engine {}: {error}. Use an extracted matching engine release, or package a local build with python3 scripts/release/package-engine.py {} --engine LIBRARY --out-dir PACKAGES and extract the archive; see the native-engine distribution documentation", engine.module, selected.display(), engine.module.trim_start_matches("std.")).into()
    })
}

fn copy_engine_inner(engine: &Engine, selected: &Path, bundle: &Path) -> Result<PathBuf> {
    let directory = selected.parent().unwrap();
    let notices = if !directory.join(engine.notices).exists()
        && directory.file_name().is_some_and(|n| n == "lib")
    {
        directory.parent().unwrap().to_path_buf()
    } else {
        directory.join(engine.notices)
    };
    let copied_notices = bundle.join(engine.notices);
    std::fs::create_dir(&copied_notices)?;
    for name in [
        "engine.json",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "THIRD-PARTY-NOTICES.md",
    ] {
        copy_file(&notices.join(name), &copied_notices.join(name))?;
    }
    copy_licenses(&notices.join("licenses"), &copied_notices.join("licenses"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(copied_notices.join("engine.json"))?)
            .map_err(|e| format!("invalid engine.json: {e}"))?;
    for (field, expected) in [
        ("format", serde_json::json!(1)),
        (
            "plenty_version",
            serde_json::json!(env!("CARGO_PKG_VERSION")),
        ),
        ("target", serde_json::json!(native_target())),
        ("abi_generation", serde_json::json!(1)),
        (
            "library",
            serde_json::json!(format!("lib/{}", engine.library)),
        ),
    ] {
        if manifest.get(field) != Some(&expected) {
            return Err(format!("engine.json {field} must be {expected}").into());
        }
    }
    for field in ["source_commit", "minimum_glibc"] {
        if !manifest
            .get(field)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            return Err(format!("engine.json requires nonempty {field}").into());
        }
    }
    let copied = bundle.join(engine.library);
    copy_file(selected, &copied)?;
    // Hash the staged bytes, then link those same bytes, not the mutable source.
    let bytes = std::fs::read(&copied)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if manifest.get("library_sha256").and_then(|v| v.as_str()) != Some(&digest) {
        return Err("engine.json library_sha256 does not match the selected library".into());
    }
    let elf = object::read::elf::ElfFile64::<object::Endianness>::parse(bytes.as_slice())?;
    let dynamic = elf.elf_dynamic_table()?;
    let soname = dynamic.iter().find(|d| d.tag == object::elf::DT_SONAME);
    if soname.map(|d| dynamic.string(d)).transpose()? != Some(engine.library.as_bytes()) {
        return Err(format!("engine SONAME must be {}", engine.library).into());
    }
    Ok(copied)
}

pub(super) fn validate_executable(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let elf = object::read::elf::ElfFile64::<object::Endianness>::parse(bytes.as_slice())?;
    let dynamic = elf.elf_dynamic_table()?;
    let mut origin = false;
    for entry in dynamic.iter() {
        if entry.tag == object::elf::DT_RPATH
            || (entry.tag == object::elf::DT_RUNPATH && dynamic.string(entry)? != b"$ORIGIN")
        {
            return Err(
                "distribution linker produced a non-$ORIGIN RUNPATH or legacy RPATH".into(),
            );
        }
        origin |= entry.tag == object::elf::DT_RUNPATH;
    }
    if !origin
        && ENGINES
            .iter()
            .any(|e| path.with_file_name(e.library).exists())
    {
        return Err("distribution linker omitted the required $ORIGIN RUNPATH".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_moves_the_complete_stage() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("app");
        let bundle = Bundle::new(&destination.join("main")).unwrap();
        let stage = bundle.path().to_owned();
        std::fs::write(stage.join("main"), b"application").unwrap();
        std::fs::create_dir(stage.join("licenses")).unwrap();
        std::fs::write(stage.join("licenses/notice"), b"notice").unwrap();

        bundle.publish().unwrap();
        assert_eq!(
            std::fs::read(destination.join("main")).unwrap(),
            b"application"
        );
        assert_eq!(
            std::fs::read(destination.join("licenses/notice")).unwrap(),
            b"notice"
        );
        assert!(!stage.exists());
    }

    #[test]
    fn publication_preserves_a_file_created_during_build() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("app");
        let bundle = Bundle::new(&destination.join("main")).unwrap();
        let stage = bundle.path().to_owned();
        std::fs::write(stage.join("main"), b"application").unwrap();
        std::fs::write(&destination, b"other build").unwrap();

        assert!(bundle.publish().is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"other build");
        assert!(!stage.exists());
    }

    #[cfg(windows)]
    #[test]
    fn publication_supports_long_unicode_paths() {
        let temp = tempfile::tempdir().unwrap();
        let mut parent = temp.path().to_owned();
        for _ in 0..8 {
            parent.push("distribution-with-a-long-name-λ");
        }
        std::fs::create_dir_all(&parent).unwrap();
        let destination = parent.join("app");
        assert!(destination.as_os_str().len() > 260);
        let bundle = Bundle::new(&destination.join("main")).unwrap();
        std::fs::write(bundle.path().join("main"), b"application").unwrap();

        bundle.publish().unwrap();
        assert_eq!(
            std::fs::read(destination.join("main")).unwrap(),
            b"application"
        );
    }

    #[test]
    fn publication_does_not_replace_a_destination_created_during_build() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("app");
        let bundle = Bundle::new(&destination.join("main")).unwrap();
        let stage = bundle.path().to_owned();
        std::fs::create_dir(&destination).unwrap();
        assert!(bundle.publish().is_err());
        assert!(destination.is_dir());
        assert!(!stage.exists());
    }
}

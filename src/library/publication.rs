//! Stage on the destination filesystem and roll back ordinary publication errors.
use std::path::{Path, PathBuf};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn validate(destinations: &[&Path], sources: &[PathBuf]) -> Result<()> {
    for path in destinations {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.file_type().is_file() {
                    return Err(format!("artifact destination {} must be a regular file, not a symlink or directory", path.display()).into());
                }
                if sources.contains(&path.canonicalize()?) {
                    return Err(format!(
                        "artifact destination {} would overwrite an input source/library",
                        path.display()
                    )
                    .into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Each rename is atomic; the multi-file set is not a crash-atomic transaction.
/// Callers must exclude concurrent writers/readers during publication.
pub(crate) fn publish(workspace: tempfile::TempDir, files: &[(PathBuf, PathBuf)]) -> Result<()> {
    let mut changed = Vec::new();
    let install: std::io::Result<()> = (|| {
        for (index, (staged, destination)) in files.iter().enumerate() {
            let backup = workspace.path().join(format!("previous-{index}"));
            let previous = match std::fs::symlink_metadata(destination) {
                Ok(metadata) if metadata.file_type().is_file() => {
                    std::fs::rename(destination, &backup)?;
                    Some(backup)
                }
                Ok(_) => {
                    return Err(std::io::Error::other(
                        "artifact destination changed to a non-regular file",
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            changed.push((destination.clone(), previous, false));
            std::fs::rename(staged, destination)?;
            changed.last_mut().unwrap().2 = true;
        }
        Ok(())
    })();
    if let Err(error) = install {
        let mut failures = Vec::new();
        for (destination, previous, installed) in changed.into_iter().rev() {
            if installed {
                if let Err(error) = std::fs::remove_file(&destination) {
                    failures.push(format!("removing {}: {error}", destination.display()));
                }
            }
            if let Some(backup) = previous {
                if let Err(error) = std::fs::rename(&backup, &destination) {
                    failures.push(format!("restoring {}: {error}", destination.display()));
                }
            }
        }
        if failures.is_empty() {
            return Err(
                format!("artifact publication failed; previous outputs restored: {error}").into(),
            );
        }
        let recovery = workspace.keep();
        return Err(format!("artifact publication failed: {error}; rollback errors: {}; recovery files retained at {}", failures.join("; "), recovery.display()).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_install_restores_old_files_and_removes_new_ones() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir_in(root.path()).unwrap();
        let existing = root.path().join("existing");
        let added = root.path().join("added");
        let last = root.path().join("last");
        std::fs::write(&existing, "old first").unwrap();
        std::fs::write(&last, "old last").unwrap();
        let first_stage = workspace.path().join("first");
        let second_stage = workspace.path().join("second");
        let missing = workspace.path().join("missing");
        std::fs::write(&first_stage, "new first").unwrap();
        std::fs::write(&second_stage, "new added").unwrap();
        let error = publish(
            workspace,
            &[
                (first_stage, existing.clone()),
                (second_stage, added.clone()),
                (missing, last.clone()),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("previous outputs restored"), "{error}");
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "old first");
        assert_eq!(std::fs::read_to_string(last).unwrap(), "old last");
        assert!(!added.exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

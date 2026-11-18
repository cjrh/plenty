//! Cooperative build locks. Keep each lock file at a stable name: deleting a
//! locked file could give a new contender a different inode to lock.
use std::fs::File;
use std::io;
use std::path::Path;

pub(crate) fn acquire(path: &Path) -> io::Result<File> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let lock = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    match lock.try_lock() {
        Ok(()) => (),
        Err(std::fs::TryLockError::WouldBlock) => {
            eprintln!("Waiting for build lock {}", path.display());
            lock.lock()?;
        }
        Err(std::fs::TryLockError::Error(error)) => return Err(error),
    }
    Ok(lock)
}

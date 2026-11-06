//! Linux advisory locks; policy and bounded cancellation waits live in Plenty.
//! Destruction only closes the File owner, never unlocking an open description
//! that another inherited descriptor may still reference.
//! Linux ABI bindings use libc (MIT OR Apache-2.0), already credited in
//! scripts/release/third_party_notices.py.
use super::Type;
use crate::text_io::{self, Error, Kind};

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod native {
    use super::*;
    use crate::{files, memory::AllocError};
    use std::os::fd::{FromRawFd, RawFd};

    fn open(path: &[u8]) -> Result<std::fs::File, Error> {
        if path.is_empty() || path.len() > 4096 || path.contains(&0) {
            return Err(Kind::InvalidInput.into());
        }
        let mut terminated = Vec::new();
        terminated
            .try_reserve_exact(path.len() + 1)
            .map_err(|_| AllocError::OutOfMemory)?;
        terminated.extend_from_slice(path);
        terminated.push(0);
        // SAFETY: terminated contains exactly one trailing NUL; open returns
        // a new descriptor. NONBLOCK prevents FIFO opens hanging before fstat.
        let fd = unsafe {
            libc::open(
                terminated.as_ptr().cast(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK,
                0o600u32,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: this is the sole owner of the successful open's descriptor.
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        if !file.metadata()?.is_file() {
            return Err(Kind::InvalidInput.into());
        }
        Ok(file)
    }

    fn try_lock(fd: RawFd, exclusive: bool) -> Result<bool, Error> {
        // SAFETY: fd is borrowed from a live File. Nonblocking flock never
        // borrows memory; the owner is closed on failure by its caller.
        let result = unsafe {
            libc::flock(
                fd,
                libc::LOCK_NB
                    | if exclusive {
                        libc::LOCK_EX
                    } else {
                        libc::LOCK_SH
                    },
            )
        };
        if result == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(error.into())
        }
    }

    #[inline(always)]
    pub(super) unsafe fn call(op: i64, a: u128, b: u128, ty: &'static Type) -> Result<u128, Error> {
        // SAFETY: compiler signatures supply a live list[u8] or File loan.
        unsafe {
            match op {
                620 => {
                    let path = super::super::filesystem::contiguous(a)?;
                    files::prepare(true, ty.variants[0].fields[0], || open(&path))
                }
                621 => Ok(try_lock(files::descriptor(a as *const _)?, b != 0)? as u128),
                _ => crate::fail("invalid file lock operation"),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{BufRead, BufReader, Write};
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        struct Fixture(std::path::PathBuf);
        impl Fixture {
            fn new(name: &str) -> Self {
                let path = std::env::temp_dir()
                    .join(format!("plenty-flock-{}-{name}", std::process::id()));
                std::fs::create_dir(&path).unwrap();
                Self(path)
            }
            fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }

        #[test]
        fn inherited_description_retains_lock_until_last_close() {
            let dir = Fixture::new("inherit");
            let path = dir.path().join("lock");
            use std::os::unix::ffi::OsStrExt;
            let owner = open(path.as_os_str().as_bytes()).unwrap();
            let fd = owner.as_raw_fd();
            assert!(try_lock(fd, true).unwrap());
            // The real API leaves CLOEXEC set. Only this test deliberately
            // inherits the descriptor to verify close-versus-unlock behavior.
            let mut command = Command::new("sh");
            command
                .args(["-c", "echo ready; read answer"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
            // SAFETY: the child hook only makes an async-signal-safe fcntl call
            // on the descriptor inherited from the still-live owner.
            unsafe {
                command.pre_exec(move || {
                    if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn().unwrap();
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(line.trim(), "ready");
            drop(owner);
            let contender = open(path.as_os_str().as_bytes()).unwrap();
            assert!(!try_lock(contender.as_raw_fd(), true).unwrap());
            child.stdin.take().unwrap().write_all(b"done\n").unwrap();
            assert!(child.wait().unwrap().success());
            assert!(try_lock(contender.as_raw_fd(), true).unwrap());
        }

        #[test]
        fn opening_preserves_contents_and_uses_private_close_on_exec_descriptor() {
            use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
            let dir = Fixture::new("open");
            let path = dir.path().join("lock");
            let owner = open(path.as_os_str().as_bytes()).unwrap();
            let mode = owner.metadata().unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
            // SAFETY: owner holds a live descriptor.
            assert_ne!(
                unsafe { libc::fcntl(owner.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
            std::fs::write(&path, b"keep this").unwrap();
            drop(open(path.as_os_str().as_bytes()).unwrap());
            assert_eq!(std::fs::read(path).unwrap(), b"keep this");
        }
    }
}

#[inline(always)]
pub(super) unsafe fn call(op: i64, a: u128, b: u128, descriptor: *const Type) -> u128 {
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    {
        // SAFETY: codegen supplies live borrowed operands and result metadata.
        text_io::result(unsafe { native::call(op, a, b, &*descriptor) })
    }
    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    {
        let _ = (op, a, b, descriptor);
        text_io::result(Err(Kind::Unsupported.into()))
    }
}

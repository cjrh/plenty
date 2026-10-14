//! Environment reads and child processes for `std.env` and `std.process`.
//! Spawning goes through Rust's `Command`, so there is no shell, the exec
//! failure is reported to the parent, and every pipe end is close-on-exec.
//! `Command` builds its argument vector with the host allocator; that short
//! transient is the one allocation here outside Plenty's fallible accounting.
use super::filesystem::{byte_list, contiguous};
use super::{wrap, Collection, OwnedValue, Type};
use crate::text_io::{self, Error, Kind};

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod native {
    use super::*;
    use std::ffi::OsStr;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// An unreaped child. `pipes` are the descriptor numbers of its piped
    /// stdin, stdout and stderr, or -1; the descriptors themselves belong to
    /// the `File` owners handed to the program.
    struct Entry {
        id: u32,
        child: Child,
        pipes: [i32; 3],
    }

    /// A handle is `id << 32 | pid`. Ids are never reused, so a handle kept
    /// after reaping cannot name a later child that received the same pid, and
    /// a signal is only ever sent to a pid this table still holds unreaped.
    static CHILDREN: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    static NEXT: AtomicU32 = AtomicU32::new(1);

    const RUNNING: u128 = 1 << 16;
    const STDIN_PIPE: u64 = 0b11;
    const CHANGE_DIR: u64 = 1 << 6;
    const CLEAR_ENV: u64 = 1 << 7;

    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
        fn poll(fds: *mut PollFd, count: u64, timeout: i32) -> i32;
    }

    fn children() -> MutexGuard<'static, Vec<Entry>> {
        CHILDREN.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn position(table: &[Entry], handle: u128) -> Result<usize, Error> {
        let id = (handle as u64 >> 32) as u32;
        table
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(Kind::InvalidInput.into())
    }

    /// The table's storage is returned with its last entry, so a program
    /// that reaped every child ends with no allocation outstanding.
    fn remove(table: &mut Vec<Entry>, at: usize) -> Entry {
        let entry = table.swap_remove(at);
        if table.is_empty() {
            *table = Vec::new();
        }
        entry
    }

    fn stdio(code: u64) -> Result<Stdio, Error> {
        match code & STDIN_PIPE {
            0 => Ok(Stdio::inherit()),
            1 => Ok(Stdio::null()),
            2 => Ok(Stdio::piped()),
            _ => Err(Kind::InvalidInput.into()),
        }
    }

    /// `spec` is NUL-terminated strings: the program, `argc` arguments, the
    /// working directory when flagged, then environment edits, `NAME=value`
    /// to set and a bare `NAME` to remove.
    fn spawn(spec: &[u8], argc: u128, flags: u64) -> Result<u128, Error> {
        let spec = spec.strip_suffix(&[0]).ok_or(Kind::InvalidInput)?;
        let mut parts = spec.split(|byte| *byte == 0);
        let mut next = || parts.next().ok_or(Kind::InvalidInput);
        let program = next()?;
        if program.is_empty() {
            return Err(Kind::InvalidInput.into());
        }
        let mut command = Command::new(OsStr::from_bytes(program));
        for _ in 0..argc as u64 {
            command.arg(OsStr::from_bytes(next()?));
        }
        if flags & CHANGE_DIR != 0 {
            command.current_dir(OsStr::from_bytes(next()?));
        }
        if flags & CLEAR_ENV != 0 {
            command.env_clear();
        }
        for edit in parts {
            match edit.iter().position(|byte| *byte == b'=') {
                Some(0) => return Err(Kind::InvalidInput.into()),
                Some(at) => {
                    command.env(
                        OsStr::from_bytes(&edit[..at]),
                        OsStr::from_bytes(&edit[at + 1..]),
                    );
                }
                None if edit.is_empty() => return Err(Kind::InvalidInput.into()),
                None => {
                    command.env_remove(OsStr::from_bytes(edit));
                }
            }
        }
        command
            .stdin(stdio(flags)?)
            .stdout(stdio(flags >> 2)?)
            .stderr(stdio(flags >> 4)?);
        let mut table = children();
        // Reserved before the child exists, so a started child is always recorded.
        table
            .try_reserve(1)
            .map_err(|_| crate::memory::AllocError::OutOfMemory)?;
        let id = NEXT.load(Ordering::Relaxed);
        if id >= 1 << 31 {
            return Err(Kind::Other.into());
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                if table.is_empty() {
                    *table = Vec::new();
                }
                return Err(error.into());
            }
        };
        NEXT.store(id + 1, Ordering::Relaxed);
        let pipes = [
            child.stdin.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            child.stdout.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            child.stderr.as_ref().map_or(-1, AsRawFd::as_raw_fd),
        ];
        let handle = (id as u128) << 32 | child.id() as u128;
        table.push(Entry { id, child, pipes });
        Ok(handle)
    }

    fn pipe(handle: u128, which: u128, ty: &'static Type) -> Result<u128, Error> {
        let mut table = children();
        let at = position(&table, handle)?;
        let child = &mut table[at].child;
        let end = match which {
            0 => child.stdin.take().map(OwnedFd::from),
            1 => child.stdout.take().map(OwnedFd::from),
            2 => child.stderr.take().map(OwnedFd::from),
            _ => None,
        };
        let end = end.ok_or(Kind::InvalidInput)?;
        crate::files::adopt(std::fs::File::from(end), which != 0, ty)
    }

    /// An exit code is 0–255, a terminating signal is 256 plus its number.
    fn wait(handle: u128, block: bool) -> Result<u128, Error> {
        let mut table = children();
        let at = position(&table, handle)?;
        let status = if block {
            let mut entry = remove(&mut table, at);
            // Other children stay usable from other threads while this one runs.
            drop(table);
            entry.child.wait()?
        } else {
            match table[at].child.try_wait()? {
                Some(status) => {
                    remove(&mut table, at);
                    status
                }
                None => return Ok(RUNNING),
            }
        };
        match (status.code(), status.signal()) {
            (Some(code), _) => Ok((code & 255) as u128),
            (None, Some(signal)) => Ok(256 + signal as u128),
            (None, None) => Err(Kind::Other.into()),
        }
    }

    fn signal(handle: u128, number: u128) -> Result<u128, Error> {
        let table = children();
        let at = position(&table, handle)?;
        if !(1..=64).contains(&number) {
            return Err(Kind::InvalidInput.into());
        }
        // SAFETY: the entry is unreaped under the lock, so its pid is still
        // this child's (possibly a zombie's) and cannot have been reused.
        if unsafe { kill(table[at].child.id() as i32, number as i32) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(0)
    }

    /// Bit 0 of `interest` asks whether stdin accepts a write, bits 1 and 2
    /// whether stdout and stderr have data or reached end of input. An error
    /// or hang-up also reports ready: the transfer that follows names it.
    fn ready(handle: u128, interest: u64, timeout: i64) -> Result<u128, Error> {
        let pipes = {
            let table = children();
            table[position(&table, handle)?].pipes
        };
        let mut fds = [0, 1, 2].map(|index| PollFd {
            fd: if interest & (1 << index) != 0 {
                pipes[index]
            } else {
                -1
            },
            events: if index == 0 { 4 } else { 1 },
            revents: 0,
        });
        if fds.iter().all(|entry| entry.fd < 0) {
            return Err(Kind::InvalidInput.into());
        }
        let timeout = i32::try_from(timeout).unwrap_or(i32::MAX).max(-1);
        // SAFETY: three initialized entries; negative descriptors are skipped.
        if unsafe { poll(fds.as_mut_ptr(), 3, timeout) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(fds
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.fd >= 0 && entry.revents != 0)
            .map(|(index, _)| 1u128 << index)
            .sum())
    }

    unsafe fn variables(ty: &'static Type) -> Result<u128, Error> {
        // SAFETY: metadata describes list[list[u8]]; the guards release a
        // partially built list.
        unsafe {
            let owner = OwnedValue {
                value: super::super::try_collection_new(ty, 0)? as u128,
                ty,
            };
            for (name, value) in std::env::vars_os() {
                for part in [name, value] {
                    let child = OwnedValue {
                        value: byte_list(part.as_bytes(), ty.key())?,
                        ty: ty.key(),
                    };
                    (*(owner.value as *mut Collection)).try_store(child.value, 0)?;
                    child.into_value();
                }
            }
            Ok(owner.into_value())
        }
    }

    pub(super) unsafe fn call(
        op: i64,
        a: u128,
        b: u128,
        c: u128,
        descriptor: &'static Type,
    ) -> Result<u128, Error> {
        let output = descriptor.variants[0].fields;
        // SAFETY: compiler signatures check each operand; list borrows end
        // before the caller releases its retained inputs.
        unsafe {
            match op {
                193 => match std::env::var_os(OsStr::from_bytes(&contiguous(a)?)) {
                    Some(value) => Ok(wrap(
                        byte_list(value.as_bytes(), output[0].variants[1].fields[0])?,
                        1,
                    )),
                    None => Ok(0),
                },
                194 => variables(output[0]),
                195 => byte_list(
                    &std::env::current_dir()?.into_os_string().into_vec(),
                    output[0],
                ),
                196 => {
                    std::env::set_current_dir(OsStr::from_bytes(&contiguous(a)?))?;
                    Ok(0)
                }
                197 => spawn(&contiguous(a)?, b, c as u64),
                198 => pipe(a, b, output[0]),
                199 => wait(a, b != 0),
                228 => signal(a, b),
                229 => ready(a, b as u64, c as i64),
                _ => crate::fail("invalid process operation"),
            }
        }
    }
}

pub(super) unsafe fn call(op: i64, a: u128, b: u128, c: u128, descriptor: *const Type) -> u128 {
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    {
        // SAFETY: codegen supplies process-lifetime output metadata.
        text_io::result(unsafe { native::call(op, a, b, c, &*descriptor) })
    }
    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    {
        let _ = (op, a, b, c, descriptor, wrap, contiguous, byte_list);
        text_io::result(Err(Kind::Unsupported.into()))
    }
}

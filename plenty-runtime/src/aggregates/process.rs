//! Environment reads and child processes for `std.env` and `std.process`.
//! Spawning goes through Rust's `Command`, so there is no shell, the exec
//! failure is reported to the parent, and every pipe end is close-on-exec.
//! `Command` builds its argument vector with the host allocator; that short
//! transient and fixed pre-exec callbacks are outside Plenty's fallible accounting.
use super::filesystem::{byte_list, contiguous};
use super::{Collection, OwnedValue, Type, wrap};
use crate::text_io::{self, Error, Kind};

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod native {
    use super::*;
    use std::ffi::{CStr, OsStr};
    use std::os::fd::{AsFd, AsRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::Duration;

    /// An unreaped child. `pipes` are the descriptor numbers of its piped
    /// stdin, stdout and stderr, or -1; the descriptors themselves belong to
    /// the `File` owners handed to the program.
    struct Entry {
        id: u32,
        child: Child,
        pipes: [i32; 3],
        isolated: bool,
    }

    /// A handle is `id << 32 | pid`. Ids are never reused, so a handle kept
    /// after reaping cannot name a later child that received the same pid, and
    /// a signal is only ever sent to a pid this table still holds unreaped.
    static CHILDREN: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    static NEXT: AtomicU32 = AtomicU32::new(1);

    const RUNNING: u128 = 1 << 16;
    const NEW_GROUP: u64 = 1 << 26;
    const NEW_SESSION: u64 = 1 << 27;
    /// Each stream code is 8 bits of `flags`: a mode in bits 0..3 and, for
    /// `REDIRECTED`, the index of its file from bit 3 (see `std.process`).
    const STREAM_BITS: u64 = 8;
    const MODE: u64 = 0b111;
    const INDEX_SHIFT: u64 = 3;
    const INHERIT: u64 = 0;
    const NULL: u64 = 1;
    const PIPED: u64 = 2;
    const REDIRECTED: u64 = 3;
    const MERGED: u64 = 4;
    const CHANGE_DIR: u64 = 1 << 24;
    const CLEAR_ENV: u64 = 1 << 25;

    /// Exec fails with ETXTBSY while any process has the program open for
    /// writing. A child forked by another thread holds a copy of each
    /// descriptor open at that moment until its exec closes them, and its
    /// parent can resume before that close, so a program this process just
    /// wrote and closed can be busy for a moment. Spawn retries after these
    /// waits, 127 ms in all, then reports the error.
    const BUSY_WAITS_MS: [u64; 7] = [1, 2, 4, 8, 16, 32, 64];
    const ETXTBSY: i32 = 26;

    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
        fn setsid() -> i32;
        fn fcntl(fd: i32, command: i32, ...) -> i32;
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

    /// Stream `stream`'s code. Only stderr may follow stdout.
    fn stream_code(flags: u64, stream: u64) -> Result<u64, Error> {
        let code = flags >> (stream * STREAM_BITS) & 0xff;
        match code & MODE {
            INHERIT | NULL | PIPED | REDIRECTED => Ok(code),
            MERGED if stream == 2 => Ok(code),
            _ => Err(Kind::InvalidInput.into()),
        }
    }

    /// The file a `REDIRECTED` code names, duplicated for the child.
    ///
    /// # Safety
    /// `files` and `inherited` are live borrowed `list[File]` values.
    unsafe fn redirected(files: u128, code: u64, write: bool) -> Result<std::fs::File, Error> {
        // SAFETY: per the contract, each entry key is a live file owner.
        unsafe {
            let list = &*(files as *const Collection);
            let at = (code >> INDEX_SHIFT) as usize;
            let file = list.entries.iter().nth(at).ok_or(Kind::InvalidInput)?.key;
            crate::files::child_stream(file as *const crate::files::File, write)
        }
    }

    /// # Safety
    /// `files` and `inherited` are live borrowed `list[File]` values.
    unsafe fn stdio(code: u64, files: u128, write: bool) -> Result<Stdio, Error> {
        Ok(match code & MODE {
            INHERIT => Stdio::inherit(),
            NULL => Stdio::null(),
            PIPED => Stdio::piped(),
            // SAFETY: forwarded from the caller.
            _ => Stdio::from(unsafe { redirected(files, code, write)? }),
        })
    }

    /// `spec` is NUL-terminated strings: the program, `argc` arguments, the
    /// working directory when flagged, then environment edits, `NAME=value`
    /// to set and a bare `NAME` to remove. `files` holds the files that
    /// redirected stream codes name.
    ///
    /// # Safety
    /// `files` and `inherited` are live borrowed `list[File]` values.
    pub(super) unsafe fn spawn(
        spec: &[u8],
        argc: u128,
        flags: u64,
        files: u128,
        inherited: &Collection,
    ) -> Result<u128, Error> {
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
        let (input, output, error) = (
            stream_code(flags, 0)?,
            stream_code(flags, 1)?,
            stream_code(flags, 2)?,
        );
        // A merged stderr shares stdout's destination. For a pipe this
        // module creates it, so both ends write to the one the Child reads.
        let mut merged_pipe = None;
        // SAFETY: `files` is live per this function's contract.
        unsafe {
            command.stdin(stdio(input, files, false)?);
            if error & MODE == MERGED {
                let (same, other) = match output & MODE {
                    INHERIT => {
                        let out = std::io::stdout().as_fd().try_clone_to_owned()?;
                        (Stdio::inherit(), Stdio::from(out))
                    }
                    NULL => (Stdio::null(), Stdio::null()),
                    PIPED => {
                        let (reader, writer) = std::io::pipe()?;
                        merged_pipe = Some(reader);
                        (Stdio::from(writer.try_clone()?), Stdio::from(writer))
                    }
                    _ => {
                        let file = redirected(files, output, true)?;
                        (Stdio::from(file.try_clone()?), Stdio::from(file))
                    }
                };
                command.stdout(same).stderr(other);
            } else {
                command
                    .stdout(stdio(output, files, true)?)
                    .stderr(stdio(error, files, true)?);
            }
        }
        match flags & (NEW_GROUP | NEW_SESSION) {
            0 => {}
            NEW_GROUP => {
                command.process_group(0);
            }
            NEW_SESSION => {
                // SAFETY: the child callback only invokes async-signal-safe
                // setsid and captures nothing. Command's exec-error pipe
                // reports setup failures and reaps the failed child.
                unsafe {
                    command.pre_exec(|| {
                        if setsid() < 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            Ok(())
                        }
                    });
                }
            }
            _ => return Err(Kind::InvalidInput.into()),
        }
        // Owners stay borrowed through exec, so none of these slots can be
        // reused by Command's internal error pipe. Parent flags never change.
        let count = inherited.entries.len();
        if count > 64 {
            return Err(Kind::InvalidInput.into());
        }
        let mut descriptors = [0i32; 64];
        for (slot, entry) in descriptors.iter_mut().zip(inherited.entries.iter()) {
            // SAFETY: the compiler checks the borrowed list[File] operand.
            *slot = unsafe { crate::files::descriptor(entry.key as *const crate::files::File)? };
            if *slot < 3 {
                return Err(Kind::InvalidInput.into());
            }
        }
        if count != 0 {
            // SAFETY: after fork this callback only uses async-signal-safe
            // fcntl and errno access, with stack data and no allocation/locks.
            // F_SETFD=2; clearing FD_CLOEXEC affects only the child's table.
            unsafe {
                command.pre_exec(move || {
                    for &fd in &descriptors[..count] {
                        if fcntl(fd, 2, 0) == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
        }
        let mut table = children();
        // Reserved before the child exists, so a started child is always recorded.
        table
            .try_reserve(1)
            .map_err(|_| crate::memory::AllocError::OutOfMemory)?;
        let id = NEXT.load(Ordering::Relaxed);
        if id >= 1 << 31 {
            return Err(Kind::Other.into());
        }
        let mut waits = BUSY_WAITS_MS.iter();
        let mut child = loop {
            match command.spawn() {
                Ok(child) => break child,
                Err(error) => {
                    if error.raw_os_error() == Some(ETXTBSY)
                        && let Some(&wait) = waits.next()
                    {
                        // The lock stays held so no thread here forks
                        // another child that could inherit a descriptor.
                        std::thread::sleep(Duration::from_millis(wait));
                        continue;
                    }
                    if table.is_empty() {
                        *table = Vec::new();
                    }
                    return Err(error.into());
                }
            }
        };
        // The parent's copies of redirected and merged write ends close here,
        // so a merged pipe reaches end of input when the child's copies close.
        drop(command);
        if let Some(reader) = merged_pipe {
            child.stdout = Some(OwnedFd::from(reader).into());
        }
        NEXT.store(id + 1, Ordering::Relaxed);
        let pipes = [
            child.stdin.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            child.stdout.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            child.stderr.as_ref().map_or(-1, AsRawFd::as_raw_fd),
        ];
        let handle = (id as u128) << 32 | child.id() as u128;
        // The table outlives every actor heap, so it grows in System.
        crate::actor_heap::outside(|| {
            table.push(Entry {
                id,
                child,
                pipes,
                isolated: flags & (NEW_GROUP | NEW_SESSION) != 0,
            })
        });
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

    fn signal(handle: u128, number: u128, group: bool) -> Result<u128, Error> {
        let table = children();
        let at = position(&table, handle)?;
        if !(1..=64).contains(&number) || (group && !table[at].isolated) {
            return Err(Kind::InvalidInput.into());
        }
        let pid = table[at].child.id() as i32;
        let target = if group { -pid } else { pid };
        // SAFETY: this unreaped child pins its PID under the table lock,
        // including after exit. Only a group created with this leader's PID
        // can be targeted; inherited groups can never reach this branch.
        if unsafe { kill(target, number as i32) } != 0 {
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
        unsafe extern "C" {
            static mut environ: *const *const std::ffi::c_char;
        }
        // SAFETY: metadata describes list[list[u8]]; the guards release a
        // partially built list. Plenty never mutates the environment, and
        // native embedders must not mutate it concurrently with runtime reads.
        // Thus environ and its NUL-terminated entries stay live throughout
        // this traversal. Copy directly into fallible storage: vars_os() first
        // builds a host snapshot whose allocations abort on exhaustion.
        unsafe {
            let owner = OwnedValue {
                value: super::super::try_collection_new(ty, 0)? as u128,
                ty,
            };
            let mut entry = environ;
            while !entry.is_null() && !(*entry).is_null() {
                let bytes = CStr::from_ptr(*entry).to_bytes();
                entry = entry.add(1);
                // Match the previous Unix vars_os parsing: ignore malformed
                // entries and look beyond the first byte for the separator.
                let Some(at) = bytes
                    .get(1..)
                    .and_then(|rest| rest.iter().position(|byte| *byte == b'='))
                    .map(|at| at + 1)
                else {
                    continue;
                };
                let (name, value) = (&bytes[..at], &bytes[at + 1..]);
                for part in [name, value] {
                    let child = OwnedValue {
                        value: byte_list(part, ty.key())?,
                        ty: ty.key(),
                    };
                    (*(owner.value as *mut Collection)).try_store(child.value, 0)?;
                    child.into_value();
                }
            }
            Ok(owner.into_value())
        }
    }

    /// Copy directly into fallible Plenty storage instead of first allocating
    /// an infallible host OsString. Plenty never mutates the process environment;
    /// foreign callers must not mutate it concurrently with runtime calls.
    unsafe fn environment_value(name: u128, ty: &'static Type) -> Result<u128, Error> {
        unsafe extern "C" {
            fn getenv(name: *const std::ffi::c_char) -> *mut std::ffi::c_char;
        }
        // SAFETY: the intrinsic receives a live list[u8].
        let mut name = unsafe { contiguous(name)? };
        if name.is_empty() || name.contains(&0) || name.contains(&b'=') {
            return Err(Kind::InvalidInput.into());
        }
        name.try_reserve(1)
            .map_err(|_| crate::memory::AllocError::OutOfMemory)?;
        name.push(0);
        // SAFETY: name is terminated; environment values stay live under the
        // no-concurrent-environment-mutation contract above.
        unsafe {
            let value = getenv(name.as_ptr().cast());
            if value.is_null() {
                Ok(0)
            } else {
                Ok(wrap(byte_list(CStr::from_ptr(value).to_bytes(), ty)?, 1))
            }
        }
    }

    unsafe fn current_directory(ty: &'static Type) -> Result<u128, Error> {
        unsafe extern "C" {
            fn getcwd(buffer: *mut std::ffi::c_char, size: usize) -> *mut std::ffi::c_char;
        }
        let mut size = 256usize;
        let mut bytes = Vec::new();
        loop {
            bytes
                .try_reserve(size - bytes.len())
                .map_err(|_| crate::memory::AllocError::OutOfMemory)?;
            bytes.resize(size, 0u8);
            // SAFETY: the buffer holds size writable bytes. On success getcwd
            // writes a terminated path into that buffer, which we copy before
            // releasing it. A supplied buffer forbids libc allocating storage.
            if !unsafe { getcwd(bytes.as_mut_ptr().cast(), size) }.is_null() {
                let path = unsafe { CStr::from_ptr(bytes.as_ptr().cast()) };
                return unsafe { byte_list(path.to_bytes(), ty) };
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(34) {
                // Linux ERANGE
                return Err(error.into());
            }
            size = size
                .checked_mul(2)
                .filter(|size| *size <= isize::MAX as usize)
                .ok_or(crate::memory::AllocError::CapacityOverflow)?;
        }
    }

    // Inlined so each operation's entry symbol keeps only its own arm.
    #[inline(always)]
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
                193 => environment_value(a, output[0].variants[1].fields[0]),
                194 => variables(output[0]),
                195 => current_directory(output[0]),
                196 => {
                    std::env::set_current_dir(OsStr::from_bytes(&contiguous(a)?))?;
                    Ok(0)
                }
                198 => pipe(a, b, output[0]),
                199 => wait(a, b != 0),
                228 => signal(a, b, c != 0),
                229 => ready(a, b as u64, c as i64),
                500 => crate::signals::listen(output[0]),
                501 => crate::files::duplicate(a as *const crate::files::File, output[0]),
                502 => Ok(crate::files::descriptor(a as *const crate::files::File)? as u128),
                _ => crate::fail("invalid process operation"),
            }
        }
    }
}

/// `__process_spawn`, the process operation with separate redirection and inheritance operands.
#[inline(always)]
pub(super) unsafe fn spawn_call(operands: [u128; 5], descriptor: *const Type) -> u128 {
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    {
        let [spec, argc, flags, files, inherited] = operands;
        let _ = descriptor;
        // SAFETY: the compiler signature supplies a live list[u8] and two live
        // borrowed list[File] operands for the duration of the call.
        text_io::result(unsafe {
            contiguous(spec).and_then(|spec| {
                native::spawn(
                    &spec,
                    argc,
                    flags as u64,
                    files,
                    &*(inherited as *const Collection),
                )
            })
        })
    }
    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    {
        let _ = (operands, descriptor);
        text_io::result(Err(Kind::Unsupported.into()))
    }
}

// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
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

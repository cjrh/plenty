//! Opaque, unbuffered file owners. No descriptor or Rust layout is public ABI.
use crate::aggregates::Type;
use crate::memory::{self, Header};
use crate::text_io::{self, Error, Kind, OpenMode};
use std::io::{Read, Seek, SeekFrom, Write};

#[repr(C)]
pub(crate) struct File {
    header: Header,
    ty: &'static Type,
    file: Option<std::fs::File>,
    writable: bool,
    readable: bool,
    skip_lf: bool,
    /// The write end of a child's stdin, or a socket: a reader that has gone
    /// must be an error value here, not a SIGPIPE that ends this program.
    pipe: bool,
}

/// Runs `write` with SIGPIPE blocked on this thread. The signal a failed
/// write raised is consumed before the mask is restored; one that was already
/// pending is left for whoever expects it.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn without_sigpipe<T>(write: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
    // glibc's sigset_t is 1024 bits; signal 13 is bit 12 of the first word.
    type Set = [u64; 16];
    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanoseconds: i64,
    }
    extern "C" {
        fn pthread_sigmask(how: i32, set: *const Set, previous: *mut Set) -> i32;
        fn sigpending(set: *mut Set) -> i32;
        fn sigtimedwait(
            set: *const Set,
            info: *mut std::ffi::c_void,
            timeout: *const Timespec,
        ) -> i32;
    }
    const SIGPIPE: u64 = 1 << 12;
    const BROKEN_PIPE: i32 = 32;
    let mut only: Set = [0; 16];
    only[0] = SIGPIPE;
    let (mut pending, mut previous): (Set, Set) = ([0; 16], [0; 16]);
    let now = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: every set is a full sigset_t; SIG_BLOCK is 0 and SIG_SETMASK 2.
    unsafe {
        sigpending(&mut pending);
        pthread_sigmask(0, &only, &mut previous);
        let result = write();
        let raised = matches!(&result, Err(error) if error.raw_os_error() == Some(BROKEN_PIPE));
        if raised && pending[0] & SIGPIPE == 0 {
            sigtimedwait(&only, std::ptr::null_mut(), &now);
        }
        pthread_sigmask(2, &previous, std::ptr::null_mut());
        result
    }
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
fn without_sigpipe<T>(write: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
    write()
}

pub(crate) fn open(path: &str, mode: &str, ty: &'static Type) -> Result<u128, Error> {
    let writable = mode != "r";
    let readable = mode == "r" || mode.ends_with('+');
    let mode = OpenMode::parse(mode).ok_or(Kind::InvalidMode)?;
    // Reserve the owner before opening/truncating anything. Path allocation in
    // open_file is also fallible and precedes the native open call.
    let pointer = memory::try_allocate::<File, u8>(0)?;
    match text_io::open_file(path, mode) {
        Ok(file) => {
            // SAFETY: fresh storage has the exact File layout and no prior value.
            unsafe {
                pointer.write(File {
                    header: Header::new(destroy),
                    ty,
                    file: Some(file),
                    writable,
                    readable,
                    skip_lf: false,
                    pipe: false,
                });
            }
            Ok(pointer as u128)
        }
        Err(error) => {
            // SAFETY: no File was initialized and no descriptor was returned.
            unsafe {
                memory::free::<File, u8>(pointer, 0);
            }
            Err(error)
        }
    }
}

/// Wraps an already open descriptor, such as one end of a child's pipe. A
/// failed owner allocation closes it.
pub(crate) fn adopt(file: std::fs::File, readable: bool, ty: &'static Type) -> Result<u128, Error> {
    prepare(readable, ty, || Ok(file))
}

/// Wraps a socket, which reads and writes through one descriptor. A peer that
/// has gone is an error value, as for a child's pipe. A failed owner
/// allocation closes the socket.
pub(crate) fn adopt_socket(file: std::fs::File, ty: &'static Type) -> Result<u128, Error> {
    let pointer = adopt(file, true, ty)?;
    // SAFETY: `adopt` initialized this owner and nothing else refers to it yet.
    unsafe {
        let owner = &mut *(pointer as *mut File);
        owner.writable = true;
        owner.pipe = true;
    }
    Ok(pointer)
}

/// Wraps a regular file opened for reading and writing, such as an unnamed
/// temporary file that is written and then read back.
pub(crate) fn adopt_read_write(file: std::fs::File, ty: &'static Type) -> Result<u128, Error> {
    let pointer = adopt(file, true, ty)?;
    // SAFETY: `adopt` initialized this owner and nothing else refers to it yet.
    unsafe {
        let owner = &mut *(pointer as *mut File);
        owner.writable = true;
        owner.pipe = false;
    }
    Ok(pointer)
}

/// Reserve owner storage before a filesystem operation creates a named object.
pub(crate) fn prepare(
    readable: bool,
    ty: &'static Type,
    open: impl FnOnce() -> Result<std::fs::File, Error>,
) -> Result<u128, Error> {
    let pointer = memory::try_allocate::<File, u8>(0)?;
    let file = match open() {
        Ok(file) => file,
        Err(error) => {
            // SAFETY: this allocation has not been initialized yet.
            unsafe {
                memory::free::<File, u8>(pointer, 0);
            }
            return Err(error);
        }
    };
    // SAFETY: fresh storage has the exact File layout and no prior value.
    unsafe {
        pointer.write(File {
            header: Header::new(destroy),
            ty,
            file: Some(file),
            writable: !readable,
            readable,
            skip_lf: false,
            pipe: !readable,
        });
    }
    Ok(pointer as u128)
}

/// Borrow only the descriptor; directory-relative operations never change a
/// file cursor or outlive the compiler's shared loan.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
pub(crate) unsafe fn descriptor(pointer: *const File) -> Result<i32, Error> {
    use std::os::fd::AsRawFd;
    unsafe {
        (*pointer)
            .file
            .as_ref()
            .map(AsRawFd::as_raw_fd)
            .ok_or_else(|| Kind::Closed.into())
    }
}

/// One `ppoll` on the descriptor for at most `timeout` nanoseconds. Interest
/// bit 0 asks for input, bit 1 for output. The result has bit 0 readable,
/// bit 1 writable, bit 2 hang-up and bit 3 error; zero means the timeout
/// elapsed. Deadlines, retries and cancellation are std.ready policy.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
pub(crate) unsafe fn wait_ready(
    pointer: *const File,
    interest: u8,
    timeout: u64,
) -> Result<u128, Error> {
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanoseconds: i64,
    }
    extern "C" {
        fn ppoll(
            fds: *mut PollFd,
            count: u64,
            timeout: *const Timespec,
            mask: *const std::ffi::c_void,
        ) -> i32;
    }
    const IN: i16 = 0x1;
    const OUT: i16 = 0x4;
    const ERR: i16 = 0x8;
    const HUP: i16 = 0x10;
    const INVALID: i16 = 0x20;
    if interest == 0 || interest > 3 {
        return Err(Kind::InvalidInput.into());
    }
    // SAFETY: the caller holds a live shared loan; only the number is used.
    let fd = unsafe { descriptor(pointer)? };
    let mut entry = PollFd {
        fd,
        events: if interest & 1 != 0 { IN } else { 0 } | if interest & 2 != 0 { OUT } else { 0 },
        revents: 0,
    };
    let span = Timespec {
        seconds: (timeout / 1_000_000_000) as i64,
        nanoseconds: (timeout % 1_000_000_000) as i64,
    };
    // SAFETY: one initialized entry and a valid relative timeout; a null mask
    // leaves the thread's signal mask unchanged.
    if unsafe { ppoll(&mut entry, 1, &span, std::ptr::null()) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if entry.revents & INVALID != 0 {
        return Err(Kind::Closed.into());
    }
    let mut ready = 0;
    for (flag, bit) in [(IN, 1), (OUT, 2), (HUP, 4), (ERR, 8)] {
        if entry.revents & flag != 0 {
            ready |= bit;
        }
    }
    Ok(ready)
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
pub(crate) unsafe fn wait_ready(_: *const File, _: u8, _: u64) -> Result<u128, Error> {
    Err(Kind::Unsupported.into())
}

pub(crate) unsafe fn close(pointer: *mut File) -> Result<u128, Error> {
    // SAFETY: the compiler holds exclusive access and supplies a live owner.
    // Taking first makes repeated close and destruction safe, even after error.
    if let Some(file) = unsafe { (*pointer).file.take() } {
        text_io::close(file)?;
    }
    Ok(0)
}

pub(crate) unsafe fn closed(pointer: *const File) -> bool {
    // SAFETY: caller holds a live shared or exclusive loan.
    unsafe { (*pointer).file.is_none() }
}

pub(crate) unsafe fn read(pointer: *mut File) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.readable {
        return Err(Kind::NotReadable.into());
    }
    Ok(text_io::read_remaining(file, &mut owner.skip_lf)? as u128)
}

pub(crate) unsafe fn readline(pointer: *mut File) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.readable {
        return Err(Kind::NotReadable.into());
    }
    Ok(text_io::read_file_line(file, &mut owner.skip_lf)? as u128)
}

pub(crate) unsafe fn read_sized(pointer: *mut File, count: i64, line: bool) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.readable {
        return Err(Kind::NotReadable.into());
    }
    if count < 0 {
        if line {
            return Ok(text_io::read_file_line(file, &mut owner.skip_lf)? as u128);
        }
        return Ok(text_io::read_remaining(file, &mut owner.skip_lf)? as u128);
    }
    Ok(text_io::read_chars(file, &mut owner.skip_lf, count as u64, line)? as u128)
}

pub(crate) unsafe fn readlines(pointer: *mut File, ty: &'static Type) -> Result<u128, Error> {
    // SAFETY: compiler supplies exclusive File access and list[str] metadata.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.readable {
        return Err(Kind::NotReadable.into());
    }
    unsafe { crate::aggregates::try_reader_lines(file, &mut owner.skip_lf, ty) }
}

pub(crate) unsafe fn write(pointer: *mut File, text: &str) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner; no user
    // callback runs during writing. Validate state even for an empty string.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.writable {
        return Err(Kind::NotWritable.into());
    }
    if !text.is_empty() {
        owner.skip_lf = false;
    }
    if owner.pipe {
        without_sigpipe(|| file.write_all(text.as_bytes()))?;
    } else {
        file.write_all(text.as_bytes())?;
    }
    Ok(text.chars().count() as u128)
}

pub(crate) unsafe fn capability(pointer: *const File, write: bool) -> Result<u128, Error> {
    // SAFETY: capability queries only inspect a live shared owner.
    let owner = unsafe { &*pointer };
    if owner.file.is_none() {
        return Err(Kind::Closed.into());
    }
    Ok(if write {
        owner.writable
    } else {
        owner.readable
    } as u128)
}

pub(crate) unsafe fn tell(pointer: *mut File) -> Result<u128, Error> {
    // SAFETY: the caller holds exclusive access to a live File owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    let offset = file.stream_position()?;
    if offset > u64::MAX >> 1 {
        return Err(Kind::InvalidInput.into());
    }
    // Text cookies reserve one bit for pending universal-newline state.
    Ok(((offset << 1) | u64::from(owner.skip_lf)) as u128)
}

pub(crate) unsafe fn seek(pointer: *mut File, cookie: u64) -> Result<u128, Error> {
    // SAFETY: the caller holds exclusive access to a live File owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    file.seek(SeekFrom::Start(cookie >> 1))?;
    // Preserve decoder state on failed seeks; replace it only after success.
    owner.skip_lf = cookie & 1 != 0;
    Ok(0)
}

/// One native read; a short count is success and zero is end of input. An
/// interrupted call surfaces as its OS error so `std.io` owns the retry policy.
pub(crate) unsafe fn read_bytes(pointer: *mut File, target: &mut [u8]) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.readable {
        return Err(Kind::NotReadable.into());
    }
    let count = file.read(target)?;
    // Byte transfers carry no pending newline translation.
    owner.skip_lf = false;
    Ok(count as u128)
}

/// One native write, which may accept only a prefix of `source`.
pub(crate) unsafe fn write_bytes(pointer: *mut File, source: &[u8]) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.writable {
        return Err(Kind::NotWritable.into());
    }
    let count = if owner.pipe {
        without_sigpipe(|| file.write(source))?
    } else {
        file.write(source)?
    };
    owner.skip_lf = false;
    Ok(count as u128)
}

/// Byte-offset seek: `whence` 0 is the start, 1 the cursor, 2 the end.
pub(crate) unsafe fn seek_bytes(
    pointer: *mut File,
    whence: u8,
    offset: i64,
) -> Result<u128, Error> {
    // SAFETY: the caller holds exclusive access to a live File owner.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    let target = match whence {
        0 => SeekFrom::Start(u64::try_from(offset).map_err(|_| Kind::InvalidInput)?),
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        _ => return Err(Kind::InvalidInput.into()),
    };
    let position = file.seek(target)?;
    owner.skip_lf = false;
    i64::try_from(position)
        .map(|position| position as u128)
        .map_err(|_| Kind::InvalidInput.into())
}

pub(crate) unsafe fn flush(pointer: *mut File, durable: bool) -> Result<u128, Error> {
    // SAFETY: the compiler holds exclusive access for this operation.
    let file = unsafe { &mut (*pointer).file };
    let file = file.as_mut().ok_or(Kind::Closed)?;
    if durable {
        file.sync_all()?;
    } else {
        file.flush()?;
    }
    Ok(0)
}

pub(crate) unsafe fn truncate(pointer: *mut File, size: Option<i64>) -> Result<u128, Error> {
    // SAFETY: a live exclusively borrowed owner is supplied by the compiler.
    let owner = unsafe { &mut *pointer };
    let file = owner.file.as_mut().ok_or(Kind::Closed)?;
    if !owner.writable {
        return Err(Kind::NotWritable.into());
    }
    let size = match size {
        Some(size) => u64::try_from(size).map_err(|_| Kind::InvalidInput)?,
        None => file.stream_position()?,
    };
    if size > i64::MAX as u64 {
        return Err(Kind::InvalidInput.into());
    }
    file.set_len(size)?;
    Ok(size as u128)
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    let file = pointer.cast::<File>();
    // SAFETY: the reference count reached zero. Rust's sole descriptor owner
    // closes on drop; drop cannot report close errors. Storage is freed once.
    unsafe {
        std::ptr::drop_in_place(file);
        memory::free::<File, u8>(file, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static TYPE: Type = Type {
        inline_range: false,
        inline_bytes: 0,
        kind: b'F',
        affine: true,
        reflexive: true,
        key: None,
        value: None,
        name: "",
        variants: &[],
        drop: None,
    };

    #[test]
    fn closed_owner_retains_releases_and_closes_without_a_descriptor() {
        let pointer = memory::try_allocate::<File, u8>(0).unwrap();
        // SAFETY: model a previously closed owner; each retain is paired with
        // one release, and no access occurs after the final release.
        unsafe {
            pointer.write(File {
                header: Header::new(destroy),
                ty: &TYPE,
                file: None,
                writable: false,
                readable: true,
                skip_lf: false,
                pipe: false,
            });
            memory::plenty_retain(pointer.cast());
            memory::plenty_release(pointer.cast());
            assert!(closed(pointer));
            assert_eq!(close(pointer).unwrap(), 0);
            memory::plenty_release(pointer.cast());
        }
    }

    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    #[test]
    fn wait_ready_reports_timeout_input_hangup_and_closed_owners() {
        use std::io::Write;
        let (reader, mut writer) = std::io::pipe().unwrap();
        let mut owner = File {
            header: Header::new(destroy),
            ty: &TYPE,
            file: Some(std::fs::File::from(std::os::fd::OwnedFd::from(reader))),
            writable: false,
            readable: true,
            skip_lf: false,
            pipe: true,
        };
        let invalid = |error: Error| matches!(error, Error::Kind(Kind::InvalidInput));
        let closed = |error: Error| matches!(error, Error::Kind(Kind::Closed));
        // SAFETY: the owner is a live local for every call.
        unsafe {
            assert_eq!(wait_ready(&owner, 1, 1_000_000).ok(), Some(0));
            writer.write_all(b"x").unwrap();
            assert_eq!(wait_ready(&owner, 1, 1_000_000).ok(), Some(1));
            drop(writer);
            assert_eq!(wait_ready(&owner, 1, 0).ok().map(|bits| bits & 4), Some(4));
            assert!(wait_ready(&owner, 0, 0).is_err_and(invalid));
            owner.file = None;
            assert!(wait_ready(&owner, 1, 0).is_err_and(closed));
        }
    }
}

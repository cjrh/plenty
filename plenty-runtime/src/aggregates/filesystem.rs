//! Linux directory streams with detached entries and fallible owned buffers.
//! Rust's read_dir/scandir contracts, without Rust DirEntry's retained descriptor
//! or its infallible Arc/PathBuf allocations. Native records never cross the ABI.
//!
//! Linux ABI bindings use the [libc crate](https://github.com/rust-lang/libc),
//! licensed MIT OR Apache-2.0.
use super::{Collection, OwnedValue, Type};
use crate::memory::{self, AllocError, Header};
use crate::text_io::{self, Error, Kind};

fn reserve(bytes: &mut Vec<u8>, count: usize) -> Result<(), Error> {
    bytes
        .len()
        .checked_add(count)
        .filter(|n| *n <= isize::MAX as usize)
        .ok_or(AllocError::CapacityOverflow)?;
    bytes
        .try_reserve(count)
        .map_err(|_| AllocError::OutOfMemory)?;
    Ok(())
}

pub(super) unsafe fn contiguous(value: u128) -> Result<Vec<u8>, Error> {
    // SAFETY: the compiler supplies a live borrowed list[u8].
    let list = unsafe { &*(value as *const Collection) };
    let mut bytes = Vec::new();
    reserve(&mut bytes, list.entries.len())?;
    bytes.extend(list.entries.iter().map(|e| e.key as u8));
    Ok(bytes)
}

pub(super) unsafe fn byte_list(bytes: &[u8], ty: &'static Type) -> Result<u128, Error> {
    // SAFETY: metadata describes list[u8]; guard releases partial construction.
    unsafe {
        let owner = OwnedValue {
            value: super::try_collection_new(ty, bytes.len())? as u128,
            ty,
        };
        let list = &mut *(owner.value as *mut Collection);
        for byte in bytes {
            list.try_insert(*byte as u128, 0)?;
        }
        Ok(owner.into_value())
    }
}

#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
mod native {
    use super::*;
    use std::ffi::{CStr, OsStr, c_char, c_void};
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    // Linux's architecture-independent statx ABI, also used by Rust's Unix fs
    // backend. Reserve all 256 bytes, including kernel extension fields.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Timestamp {
        seconds: i64,
        nanos: u32,
        reserved: i32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Stat {
        mask: u32,
        block_size: u32,
        attributes: u64,
        links: u32,
        uid: u32,
        gid: u32,
        mode: u16,
        spare: u16,
        inode: u64,
        size: u64,
        blocks: u64,
        attributes_mask: u64,
        accessed: Timestamp,
        created: Timestamp,
        changed: Timestamp,
        modified: Timestamp,
        rdev_major: u32,
        rdev_minor: u32,
        dev_major: u32,
        dev_minor: u32,
        mount_id: u64,
        dio_memory_align: u32,
        dio_offset_align: u32,
        extensions: [u64; 12],
    }
    const _: () = {
        assert!(size_of::<Stat>() == 256);
        assert!(std::mem::offset_of!(Stat, accessed) == 64);
    };

    #[repr(C)]
    struct Dirent {
        inode: u64,
        offset: i64,
        record_length: u16,
        kind: u8,
        name: [c_char; 256],
    }
    const _: () = {
        assert!(size_of::<Dirent>() == size_of::<libc::dirent64>());
        assert!(align_of::<Dirent>() == align_of::<libc::dirent64>());
        assert!(std::mem::offset_of!(Dirent, name) == std::mem::offset_of!(libc::dirent64, d_name));
        assert!(size_of::<Stat>() == size_of::<libc::statx>());
        assert!(
            std::mem::offset_of!(Stat, accessed) == std::mem::offset_of!(libc::statx, stx_atime)
        );
    };

    unsafe extern "C" {
        fn opendir(path: *const c_char) -> *mut c_void;
        fn readdir64(dir: *mut c_void) -> *const Dirent;
        fn closedir(dir: *mut c_void) -> i32;
        fn __errno_location() -> *mut i32;
        fn readlink(path: *const c_char, buffer: *mut u8, size: usize) -> isize;
        fn statx(fd: i32, path: *const c_char, flags: i32, mask: u32, stat: *mut Stat) -> i32;
        fn openat(fd: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn mkdirat(fd: i32, path: *const c_char, mode: u32) -> i32;
        fn unlinkat(fd: i32, path: *const c_char, flags: i32) -> i32;
        fn renameat2(from: i32, old: *const c_char, to: i32, new: *const c_char, flags: u32)
        -> i32;
        fn linkat(from: i32, old: *const c_char, to: i32, new: *const c_char, flags: i32) -> i32;
        fn fdopendir(fd: i32) -> *mut c_void;
    }

    struct Stream(*mut c_void);
    impl Stream {
        fn close(self) -> Result<(), Error> {
            // SAFETY: disarm Drop before consuming the sole native handle.
            let pointer = self.0;
            std::mem::forget(self);
            if unsafe { closedir(pointer) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(())
        }
    }
    impl Drop for Stream {
        fn drop(&mut self) {
            // SAFETY: this guard owns one successful opendir, closed once.
            unsafe {
                closedir(self.0);
            }
        }
    }

    #[repr(C)]
    struct Directory {
        header: Header,
        root: Vec<u8>,
        stream: Option<Stream>,
    }
    #[repr(C)]
    struct Entry {
        header: Header,
        path: Vec<u8>,
        name_start: usize,
        kind: u8,
    }
    #[repr(C)]
    struct Metadata {
        header: Header,
        data: Stat,
    }

    unsafe extern "C" fn destroy_directory(pointer: *mut Header) {
        // SAFETY: the last owner has gone; drop closes any active stream.
        unsafe {
            std::ptr::drop_in_place(pointer.cast::<Directory>());
            memory::free::<Directory, u8>(pointer.cast(), 0);
        }
    }
    unsafe extern "C" fn destroy_entry(pointer: *mut Header) {
        // SAFETY: the last detached entry owner has gone.
        unsafe {
            std::ptr::drop_in_place(pointer.cast::<Entry>());
            memory::free::<Entry, u8>(pointer.cast(), 0);
        }
    }
    unsafe extern "C" fn destroy_metadata(pointer: *mut Header) {
        // SAFETY: the last metadata observation owner has gone.
        unsafe {
            std::ptr::drop_in_place(pointer.cast::<Metadata>());
            memory::free::<Metadata, u8>(pointer.cast(), 0);
        }
    }

    fn terminated(mut bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
        if bytes.contains(&0) {
            return Err(Kind::InvalidInput.into());
        }
        reserve(&mut bytes, 1)?;
        bytes.push(0);
        Ok(bytes)
    }

    fn leaf(bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
        if bytes.is_empty() || matches!(bytes.as_slice(), b"." | b"..") || bytes.contains(&b'/') {
            return Err(Kind::InvalidInput.into());
        }
        terminated(bytes)
    }

    const DIRECTORY: i32 = libc::O_DIRECTORY;
    const NOFOLLOW: i32 = libc::O_NOFOLLOW;
    const CLOEXEC: i32 = libc::O_CLOEXEC;

    fn open_at(fd: i32, path: &[u8], flags: i32) -> Result<std::fs::File, Error> {
        // SAFETY: callers provide a terminated buffer; ownership transfers once.
        let result = unsafe { openat(fd, path.as_ptr().cast(), flags | CLOEXEC, 0o600u32) };
        if result < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { std::fs::File::from_raw_fd(result) })
    }

    fn checked(status: i32) -> Result<u128, Error> {
        if status == 0 {
            Ok(0)
        } else {
            Err(std::io::Error::last_os_error().into())
        }
    }

    fn directory_entries(fd: i32) -> Result<u128, Error> {
        let pointer = memory::try_allocate::<Directory, u8>(0)?;
        let result = (|| {
            let file = open_at(fd, b".\0", DIRECTORY)?;
            // fdopendir takes ownership only on success.
            let stream = unsafe { fdopendir(file.as_raw_fd()) };
            if stream.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let _ = file.into_raw_fd();
            Ok(Stream(stream))
        })();
        match result {
            Ok(stream) => {
                unsafe {
                    pointer.write(Directory {
                        header: Header::new(destroy_directory),
                        root: Vec::new(),
                        stream: Some(stream),
                    });
                }
                Ok(pointer as u128)
            }
            Err(error) => {
                unsafe {
                    memory::free::<Directory, u8>(pointer, 0);
                }
                Err(error)
            }
        }
    }

    // Use procfs for unprivileged O_TMPFILE publication. AT_EMPTY_PATH would
    // require CAP_DAC_READ_SEARCH. The source fd remains owned for this call.
    fn publish(source: i32, destination: i32, name: &[u8]) -> Result<u128, Error> {
        let mut path = [0u8; 64];
        let prefix = b"/proc/self/fd/";
        path[..prefix.len()].copy_from_slice(prefix);
        let mut digits = [0u8; 10];
        let mut count = 0;
        let mut number = source as u32;
        loop {
            digits[count] = b'0' + (number % 10) as u8;
            count += 1;
            number /= 10;
            if number == 0 {
                break;
            }
        }
        for index in 0..count {
            path[prefix.len() + index] = digits[count - index - 1];
        }
        checked(unsafe {
            linkat(
                -100,
                path.as_ptr().cast(),
                destination,
                name.as_ptr().cast(),
                0x400,
            )
        })
    }

    // Inlined so each operation's entry symbol keeps only its own arm.
    #[inline(always)]
    pub(super) unsafe fn rooted_call(
        op: i64,
        a: u128,
        b: u128,
        c: u128,
        ty: &'static Type,
    ) -> Result<u128, Error> {
        // SAFETY: signatures supply live File loans and byte lists; none of
        // these descriptor-relative operations changes an existing cursor.
        unsafe {
            if op == 260 {
                let mut path = contiguous(a)?;
                // A trailing slash makes Linux resolve a directory symlink
                // before applying O_NOFOLLOW. Strip separators but preserve /.
                while path.len() > 1 && path.last() == Some(&b'/') {
                    path.pop();
                }
                let path = terminated(path)?;
                return crate::files::adopt(
                    open_at(-100, &path, DIRECTORY | NOFOLLOW)?,
                    true,
                    ty.variants[0].fields[0],
                );
            }
            let fd = crate::files::descriptor(a as *const crate::files::File)?;
            match op {
                261 | 262 => {
                    let name = leaf(contiguous(b)?)?;
                    // O_NONBLOCK avoids hanging on a FIFO before its type is
                    // checked. The flag is harmless for regular files.
                    let file = open_at(
                        fd,
                        &name,
                        NOFOLLOW
                            | if op == 261 {
                                DIRECTORY
                            } else {
                                libc::O_NONBLOCK
                            },
                    )?;
                    if op == 262 && !file.metadata()?.is_file() {
                        return Err(Kind::InvalidInput.into());
                    }
                    crate::files::adopt(file, true, ty.variants[0].fields[0])
                }
                263..=265 => {
                    let name = leaf(contiguous(b)?)?;
                    checked(match op {
                        263 => mkdirat(fd, name.as_ptr().cast(), 0o700),
                        264 => unlinkat(fd, name.as_ptr().cast(), 0),
                        _ => unlinkat(fd, name.as_ptr().cast(), 0x200),
                    })
                }
                266 => {
                    let old = leaf(contiguous(b)?)?;
                    let new = leaf(contiguous(c)?)?;
                    checked(renameat2(
                        fd,
                        old.as_ptr().cast(),
                        fd,
                        new.as_ptr().cast(),
                        1,
                    ))
                }
                267 => directory_entries(fd),
                268 => {
                    let file = open_at(fd, b".\0", libc::O_TMPFILE | libc::O_RDWR)?;
                    crate::files::adopt_read_write(file, ty.variants[0].fields[0])
                }
                269 => {
                    let target = crate::files::descriptor(b as *const crate::files::File)?;
                    let name = leaf(contiguous(c)?)?;
                    publish(fd, target, &name)
                }
                270 => {
                    let parent = crate::files::descriptor(b as *const crate::files::File)?;
                    let name = leaf(contiguous(c)?)?;
                    let original = open_at(fd, b".\0", DIRECTORY)?.metadata()?;
                    let current = open_at(parent, &name, DIRECTORY | NOFOLLOW)?.metadata()?;
                    Ok(
                        (original.ino() == current.ino() && original.dev() == current.dev())
                            as u128,
                    )
                }
                271 => crate::files::adopt(
                    open_at(fd, b".\0", DIRECTORY)?,
                    true,
                    ty.variants[0].fields[0],
                ),
                272 => {
                    let name = leaf(contiguous(b)?)?;
                    crate::files::prepare(true, ty.variants[0].fields[0], || {
                        checked(mkdirat(fd, name.as_ptr().cast(), 0o700))?;
                        match open_at(fd, &name, DIRECTORY | NOFOLLOW) {
                            Ok(file) => Ok(file),
                            Err(error) => {
                                // Name and owner allocations precede mkdir;
                                // rollback must work under allocator exhaustion.
                                unlinkat(fd, name.as_ptr().cast(), 0x200);
                                Err(error)
                            }
                        }
                    })
                }
                // AT_EMPTY_PATH: the open file itself, however it was reached,
                // so the observation cannot be redirected by a renamed path.
                273 => owned_metadata(stat_at(fd, b"\0", 0x1000)?),
                _ => unreachable!("rooted filesystem opcode"),
            }
        }
    }

    unsafe fn open(value: u128) -> Result<u128, Error> {
        open_bytes(unsafe { contiguous(value)? })
    }

    fn open_bytes(bytes: Vec<u8>) -> Result<u128, Error> {
        let mut root = terminated(bytes)?;
        let owner = memory::try_allocate::<Directory, u8>(0)?;
        // SAFETY: checked NUL-terminated path. Owner storage precedes opening.
        let stream = unsafe { opendir(root.as_ptr().cast()) };
        if stream.is_null() {
            let error = std::io::Error::last_os_error();
            unsafe {
                memory::free::<Directory, u8>(owner, 0);
            }
            return Err(error.into());
        }
        root.pop();
        unsafe {
            owner.write(Directory {
                header: Header::new(destroy_directory),
                root,
                stream: Some(Stream(stream)),
            });
        }
        Ok(owner as u128)
    }

    pub(super) fn join(root: &[u8], name: &[u8]) -> Result<Vec<u8>, Error> {
        if Path::new(OsStr::from_bytes(name)).is_absolute() {
            return copy(name);
        }
        let mut path = copy(root)?;
        if !path.is_empty() && !path.ends_with(b"/") {
            reserve(&mut path, 1)?;
            path.push(b'/');
        }
        reserve(&mut path, name.len())?;
        path.extend_from_slice(name);
        Ok(path)
    }

    fn copy(bytes: &[u8]) -> Result<Vec<u8>, Error> {
        let mut owned = Vec::new();
        reserve(&mut owned, bytes.len())?;
        owned.extend_from_slice(bytes);
        Ok(owned)
    }

    /// EOF and any advance/allocation failure terminate and close the stream.
    unsafe fn next(owner: &mut Directory) -> Result<u128, Error> {
        // SAFETY: the caller holds exclusive directory access.
        unsafe {
            next_with(owner, |stream| {
                *__errno_location() = 0;
                let entry = readdir64(stream);
                if entry.is_null() && *__errno_location() != 0 {
                    Err(std::io::Error::last_os_error().into())
                } else {
                    Ok(entry)
                }
            })
        }
    }

    unsafe fn next_with(
        owner: &mut Directory,
        mut read: impl FnMut(*mut c_void) -> Result<*const Dirent, Error>,
    ) -> Result<u128, Error> {
        let Some(stream) = &owner.stream else {
            return Ok(super::super::wrap(0, 0));
        };
        let result = (|| {
            loop {
                let entry = read(stream.0)?;
                if entry.is_null() {
                    return Ok(0);
                }
                // SAFETY: libc's result remains live until the next readdir call.
                // Copy all data before advancing, never retain the native record.
                let kind = unsafe { (*entry).kind };
                // A native record can be shorter than the declared d_name
                // array. Form only a pointer to its trailing NUL-terminated
                // bytes, never a reference spanning the entire Rust record.
                let name =
                    unsafe { CStr::from_ptr(std::ptr::addr_of!((*entry).name).cast()) }.to_bytes();
                if matches!(name, b"." | b"..") {
                    continue;
                }
                let path = join(&owner.root, name)?;
                let name_start = path.len() - name.len();
                let result = memory::try_allocate::<Entry, u8>(0)?;
                unsafe {
                    result.write(Entry {
                        header: Header::new(destroy_entry),
                        path,
                        name_start,
                        kind,
                    });
                }
                return Ok(super::super::wrap(result as u128, 1));
            }
        })();
        if !matches!(result, Ok(value) if value != 0) {
            owner.stream.take();
        }
        result
    }

    fn stat(path: &[u8], follow: bool) -> Result<Stat, Error> {
        #[cfg(test)]
        STAT_CALLS.with(|count| count.set(count.get() + 1));
        let path = terminated(copy(path)?)?;
        // AT_FDCWD preserves relative lookup.
        stat_at(-100, &path, if follow { 0 } else { 0x100 })
    }

    fn stat_at(fd: i32, path: &[u8], flags: i32) -> Result<Stat, Error> {
        let mut data = Stat::default();
        // SAFETY: statx writes the stable 256-byte record, using a checked
        // NUL-terminated native path. Unlike std::fs::metadata's long-path
        // CString fallback, all Rust allocation here is fallible.
        if unsafe { statx(fd, path.as_ptr().cast(), flags, 0xfff, &mut data) } != 0 {
            let error = std::io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(38) {
                Kind::Unsupported.into()
            } else {
                error.into()
            });
        }
        if data.mask & 0x39b != 0x39b {
            return Err(Kind::Unsupported.into());
        }
        Ok(data)
    }

    #[cfg(test)]
    thread_local! { static STAT_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

    fn metadata(path: &[u8], follow: bool) -> Result<u128, Error> {
        owned_metadata(stat(path, follow)?)
    }

    fn owned_metadata(data: Stat) -> Result<u128, Error> {
        let owner = memory::try_allocate::<Metadata, u8>(0)?;
        unsafe {
            owner.write(Metadata {
                header: Header::new(destroy_metadata),
                data,
            });
        }
        Ok(owner as u128)
    }

    fn kind(mode: u16) -> u8 {
        match mode >> 12 {
            8 => 8,
            4 => 4,
            10 => 10,
            _ => 0,
        }
    }

    fn file_type(entry: &Entry) -> Result<u128, Error> {
        if entry.kind != 0 {
            return Ok(match entry.kind {
                8 | 4 | 10 => entry.kind as u128,
                _ => 0,
            });
        }
        Ok(kind(stat(&entry.path, false)?.mode) as u128)
    }

    fn field(data: &Stat, index: u8) -> u128 {
        let value = match index {
            0 => kind(data.mode) as u64,
            1 => data.size,
            2 => (data.mode & 0o222 == 0) as u64,
            3 => data.mode as u64,
            4 => data.uid as u64,
            5 => data.gid as u64,
            // Linux makedev encoding, matching Unix MetadataExt::dev.
            6 => {
                ((data.dev_major as u64 & 0xfffff000) << 32)
                    | ((data.dev_major as u64 & 0xfff) << 8)
                    | ((data.dev_minor as u64 & 0xffffff00) << 12)
                    | (data.dev_minor as u64 & 0xff)
            }
            7 => data.inode,
            8..=16 => {
                let (mask, time) = match (index - 8) / 3 {
                    0 => (0x40, data.modified),
                    1 => (0x20, data.accessed),
                    _ => (0x800, data.created),
                };
                if data.mask & mask == 0 {
                    0
                } else {
                    match (index - 8) % 3 {
                        0 => 1,
                        1 => time.seconds as u64,
                        _ => time.nanos as u64,
                    }
                }
            }
            17 => data.changed.seconds as u64,
            18 => data.changed.nanos as u64,
            _ => 0,
        };
        value as u128
    }

    fn read_link(path: Vec<u8>) -> Result<Vec<u8>, Error> {
        let path = terminated(path)?;
        let mut bytes = Vec::new();
        let mut size = 256usize;
        loop {
            reserve(&mut bytes, size)?;
            bytes.resize(size, 0);
            // SAFETY: checked path and writable buffer. A full buffer may have
            // truncated a changing target; grow and retry rather than return it.
            let count = unsafe { readlink(path.as_ptr().cast(), bytes.as_mut_ptr(), bytes.len()) };
            if count < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if (count as usize) < bytes.len() {
                bytes.truncate(count as usize);
                return Ok(bytes);
            }
            bytes.clear();
            size = size.checked_mul(2).ok_or(AllocError::CapacityOverflow)?;
        }
    }

    // Inlined so each operation's entry symbol keeps only its own arm.
    #[inline(always)]
    pub(super) unsafe fn call(
        op: i64,
        a: u128,
        b: u128,
        descriptor: &'static Type,
    ) -> Result<u128, Error> {
        // SAFETY: compiler signatures check owners and list metadata; all raw
        // borrows end before the caller releases its temporary retained inputs.
        unsafe {
            match op {
                150 => open(a),
                151 => next(&mut *(a as *mut Directory)),
                152 => {
                    if let Some(stream) = (*(a as *mut Directory)).stream.take() {
                        stream.close()?;
                    }
                    Ok(0)
                }
                153 => Ok((*(a as *const Directory)).stream.is_none() as u128),
                154 | 155 => {
                    let entry = &*(a as *const Entry);
                    byte_list(
                        if op == 154 {
                            &entry.path[entry.name_start..]
                        } else {
                            &entry.path
                        },
                        descriptor.variants[0].fields[0],
                    )
                }
                156 => file_type(&*(a as *const Entry)),
                157 => metadata(&(*(a as *const Entry)).path, false),
                158 => metadata(&contiguous(a)?, b != 0),
                159 => Ok(field(&(*(a as *const Metadata)).data, b as u8)),
                160 => byte_list(
                    &read_link(contiguous(a)?)?,
                    descriptor.variants[0].fields[0],
                ),
                161 => byte_list(
                    &join(&contiguous(a)?, &contiguous(b)?)?,
                    descriptor.variants[0].fields[0],
                ),
                162 => {
                    let bytes = contiguous(a)?;
                    let path = Path::new(OsStr::from_bytes(&bytes));
                    let result = if b == 0 {
                        path.parent().map(Path::as_os_str)
                    } else {
                        path.file_name()
                    };
                    match result {
                        None => Ok(0),
                        Some(part) => Ok(super::super::wrap(
                            byte_list(
                                part.as_bytes(),
                                descriptor.variants[0].fields[0].variants[1].fields[0],
                            )?,
                            1,
                        )),
                    }
                }
                163 => {
                    let bytes = contiguous(a)?;
                    let ty = descriptor.variants[0].fields[0];
                    let owner = OwnedValue {
                        value: super::super::try_collection_new(ty, 0)? as u128,
                        ty,
                    };
                    for component in Path::new(OsStr::from_bytes(&bytes)).components() {
                        let child = OwnedValue {
                            value: byte_list(component.as_os_str().as_bytes(), ty.key())?,
                            ty: ty.key(),
                        };
                        (*(owner.value as *mut Collection)).try_store(child.value, 0)?;
                        child.into_value();
                    }
                    Ok(owner.into_value())
                }
                164 => {
                    let bytes = contiguous(a)?;
                    let mut escaped = Vec::new();
                    reserve(
                        &mut escaped,
                        bytes
                            .len()
                            .checked_mul(4)
                            .ok_or(AllocError::CapacityOverflow)?,
                    )?;
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    for byte in bytes {
                        match byte {
                            b'\\' | b'"' => escaped.extend_from_slice(&[b'\\', byte]),
                            0x20..=0x7e => escaped.push(byte),
                            _ => escaped.extend_from_slice(&[
                                b'\\',
                                b'x',
                                HEX[(byte >> 4) as usize],
                                HEX[(byte & 15) as usize],
                            ]),
                        }
                    }
                    Ok(crate::strings::try_new(
                        std::str::from_utf8(&escaped).expect("ASCII escapes"),
                    )? as u128)
                }
                _ => unreachable!("filesystem opcode"),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::MetadataExt;
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

        struct Fixture(std::path::PathBuf);
        impl Fixture {
            fn new() -> Self {
                let root = std::env::temp_dir().join(format!(
                    "plenty-native-fs-{}-{}",
                    std::process::id(),
                    SEQUENCE.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir(&root).unwrap();
                Self(root)
            }
            fn bytes(&self) -> Vec<u8> {
                self.0.as_os_str().as_bytes().to_vec()
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }

        struct Owner(u128);
        impl Drop for Owner {
            fn drop(&mut self) {
                // SAFETY: each test wraps one fresh owner and releases it once.
                unsafe {
                    memory::plenty_release(self.0 as *mut Header);
                }
            }
        }

        #[test]
        fn rooted_leaf_validation_preserves_bytes_without_navigation() {
            for invalid in [b"".as_slice(), b".", b"..", b"a/b", b"a\0b"] {
                assert!(matches!(
                    leaf(invalid.to_vec()),
                    Err(Error::Kind(Kind::InvalidInput))
                ));
            }
            assert_eq!(leaf(b"raw\xff".to_vec()).unwrap(), b"raw\xff\0");
        }

        #[test]
        fn rooted_no_follow_rejects_directory_link_and_anonymous_publish_refuses_collision() {
            let fixture = Fixture::new();
            let parent = std::fs::File::open(&fixture.0).unwrap();
            std::fs::create_dir(fixture.0.join("directory")).unwrap();
            std::os::unix::fs::symlink("directory", fixture.0.join("link")).unwrap();
            assert!(open_at(parent.as_raw_fd(), b"link\0", DIRECTORY | NOFOLLOW).is_err());
            let temporary =
                open_at(parent.as_raw_fd(), b".\0", libc::O_TMPFILE | libc::O_RDWR).unwrap();
            std::fs::write(fixture.0.join("collision"), b"keep").unwrap();
            assert!(
                matches!(publish(temporary.as_raw_fd(), parent.as_raw_fd(), b"collision\0"), Err(Error::System(error)) if error.raw_os_error() == Some(17))
            );
            assert_eq!(std::fs::read(fixture.0.join("collision")).unwrap(), b"keep");
            publish(temporary.as_raw_fd(), parent.as_raw_fd(), b"published\0").unwrap();
            assert!(fixture.0.join("published").is_file());
        }

        #[test]
        fn detached_entries_known_types_and_unknown_type_fallback() {
            let fixture = Fixture::new();
            std::fs::write(fixture.0.join("file"), b"abc").unwrap();
            let directory = Owner(open_bytes(fixture.bytes()).unwrap());
            // SAFETY: tests keep the owners alive, with exclusive access during
            // advance and only shared entry access after stream destruction.
            unsafe {
                let value = next(&mut *(directory.0 as *mut Directory)).unwrap();
                assert_eq!(value >> 64, 1);
                let entry = Owner(value as u64 as u128);
                assert_eq!(next(&mut *(directory.0 as *mut Directory)).unwrap(), 0);
                assert!((*(directory.0 as *const Directory)).stream.is_none());
                drop(directory);
                let entry = &mut *(entry.0 as *mut Entry);
                assert_eq!(&entry.path[entry.name_start..], b"file");
                STAT_CALLS.with(|count| count.set(0));
                entry.kind = 8;
                assert_eq!(file_type(entry).unwrap(), 8);
                assert_eq!(STAT_CALLS.with(|count| count.get()), 0);
                entry.kind = 0;
                assert_eq!(file_type(entry).unwrap(), 8);
                assert_eq!(STAT_CALLS.with(|count| count.get()), 1);
                std::fs::remove_file(fixture.0.join("file")).unwrap();
                assert!(matches!(file_type(entry), Err(Error::System(_))));
            }
        }

        #[test]
        fn advance_error_is_not_eof_and_closes_before_returning() {
            let fixture = Fixture::new();
            let directory = Owner(open_bytes(fixture.bytes()).unwrap());
            // SAFETY: an injected fetch failure does not modify the native stream.
            unsafe {
                let owner = &mut *(directory.0 as *mut Directory);
                let failure =
                    next_with(owner, |_| Err(std::io::Error::from_raw_os_error(5).into()));
                assert!(matches!(failure, Err(Error::System(e)) if e.raw_os_error() == Some(5)));
                assert!(owner.stream.is_none());
                assert_eq!(next(owner).unwrap(), 0);
            }
        }

        #[test]
        fn statx_fields_match_rust_and_preserve_negative_fractional_timestamps() {
            let fixture = Fixture::new();
            let path = fixture.0.join("file");
            std::fs::write(&path, b"abc").unwrap();
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH - std::time::Duration::from_millis(500)),
            )
            .unwrap();
            let data = stat(path.as_os_str().as_bytes(), true).unwrap();
            let baseline = std::fs::metadata(&path).unwrap();
            for (index, expected) in [
                (1, baseline.len()),
                (3, baseline.mode() as u64),
                (4, baseline.uid() as u64),
                (5, baseline.gid() as u64),
                (6, baseline.dev()),
                (7, baseline.ino()),
            ] {
                assert_eq!(field(&data, index), expected as u128);
            }
            assert_eq!(field(&data, 8), 1);
            assert_eq!(field(&data, 9) as u64 as i64, -1);
            assert_eq!(field(&data, 10), 500_000_000);
            assert_eq!(field(&data, 14) != 0, baseline.created().is_ok());
        }
    }
}

// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(super) unsafe fn call(op: i64, a: u128, b: u128, descriptor: *const Type) -> u128 {
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    {
        // SAFETY: codegen supplies process-lifetime output metadata.
        let result = unsafe { native::call(op, a, b, &*descriptor) };
        if matches!(op, 153 | 159) {
            return result.expect("infallible filesystem observation");
        }
        text_io::result(result)
    }
    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    {
        let _ = (a, b, descriptor);
        if matches!(op, 153 | 159) {
            return 0;
        }
        text_io::result(Err(Kind::Unsupported.into()))
    }
}

// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(super) unsafe fn rooted_call(
    op: i64,
    a: u128,
    b: u128,
    c: u128,
    descriptor: *const Type,
) -> u128 {
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    {
        text_io::result(unsafe { native::rooted_call(op, a, b, c, &*descriptor) })
    }
    #[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
    {
        let _ = (op, a, b, c, descriptor);
        text_io::result(Err(Kind::Unsupported.into()))
    }
}

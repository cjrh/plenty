//! Opaque, unbuffered file owners. No descriptor or Rust layout is public ABI.
use crate::aggregates::Type;
use crate::memory::{self, Header};
use crate::text_io::{self, Error, OpenMode};
use std::io::{Seek, SeekFrom, Write};

#[repr(C)]
pub(crate) struct File {
    header: Header,
    ty: &'static Type,
    file: Option<std::fs::File>,
    writable: bool,
    readable: bool,
    skip_lf: bool,
}

pub(crate) fn open(path: &str, mode: &str, ty: &'static Type) -> Result<u128, Error> {
    let writable = mode != "r";
    let readable = mode == "r" || mode.ends_with('+');
    let mode = match mode {
        "r" => OpenMode::Read,
        "w" => OpenMode::Replace,
        "a" => OpenMode::Append,
        "x" => OpenMode::CreateNew,
        "r+" => OpenMode::ReadWrite,
        "w+" => OpenMode::ReplaceRead,
        "a+" => OpenMode::AppendRead,
        "x+" => OpenMode::CreateNewRead,
        _ => return Err(std::io::ErrorKind::InvalidInput.into()),
    };
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
    if !owner.readable {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
    Ok(text_io::read_remaining(file, &mut owner.skip_lf)? as u128)
}

pub(crate) unsafe fn readline(pointer: *mut File) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner.
    let owner = unsafe { &mut *pointer };
    if !owner.readable {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
    Ok(text_io::read_file_line(file, &mut owner.skip_lf)? as u128)
}

pub(crate) unsafe fn read_sized(pointer: *mut File, count: i64, line: bool) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner.
    let owner = unsafe { &mut *pointer };
    if !owner.readable {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
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
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
    if !owner.readable {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    unsafe { crate::aggregates::try_reader_lines(file, &mut owner.skip_lf, ty) }
}

pub(crate) unsafe fn write(pointer: *mut File, text: &str) -> Result<u128, Error> {
    // SAFETY: the compiler supplies a live exclusively borrowed owner; no user
    // callback runs during writing. Validate state even for an empty string.
    let owner = unsafe { &mut *pointer };
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
    if !owner.writable {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    if !text.is_empty() {
        owner.skip_lf = false;
    }
    file.write_all(text.as_bytes())?;
    Ok(text.chars().count() as u128)
}

pub(crate) unsafe fn capability(pointer: *const File, write: bool) -> Result<u128, Error> {
    // SAFETY: capability queries only inspect a live shared owner.
    let owner = unsafe { &*pointer };
    if owner.file.is_none() {
        return Err(std::io::ErrorKind::NotConnected.into());
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
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
    let offset = file.stream_position()?;
    if offset > u64::MAX >> 1 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    // Text cookies reserve one bit for pending universal-newline state.
    Ok(((offset << 1) | u64::from(owner.skip_lf)) as u128)
}

pub(crate) unsafe fn seek(pointer: *mut File, cookie: u64) -> Result<u128, Error> {
    // SAFETY: the caller holds exclusive access to a live File owner.
    let owner = unsafe { &mut *pointer };
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
    file.seek(SeekFrom::Start(cookie >> 1))?;
    // Preserve decoder state on failed seeks; replace it only after success.
    owner.skip_lf = cookie & 1 != 0;
    Ok(0)
}

pub(crate) unsafe fn flush(pointer: *mut File, durable: bool) -> Result<u128, Error> {
    // SAFETY: the compiler holds exclusive access for this operation.
    let file = unsafe { &mut (*pointer).file };
    let file = file.as_mut().ok_or(std::io::ErrorKind::NotConnected)?;
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
    let file = owner
        .file
        .as_mut()
        .ok_or(std::io::ErrorKind::NotConnected)?;
    if !owner.writable {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    let size = match size {
        Some(size) => u64::try_from(size).map_err(|_| std::io::ErrorKind::InvalidInput)?,
        None => file.stream_position()?,
    };
    if size > i64::MAX as u64 {
        return Err(std::io::ErrorKind::InvalidInput.into());
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
        kind: b'F',
        affine: true,
        reflexive: true,
        key: None,
        value: None,
        name: "",
        variants: &[],
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
            });
            memory::plenty_retain(pointer.cast());
            memory::plenty_release(pointer.cast());
            assert!(closed(pointer));
            assert_eq!(close(pointer).unwrap(), 0);
            memory::plenty_release(pointer.cast());
        }
    }
}

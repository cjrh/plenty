//! Opaque, unbuffered file owners. No descriptor or Rust layout is public ABI.
use crate::aggregates::Type;
use crate::memory::{self, Header};
use crate::text_io::{self, Error, OpenMode};
use std::io::Write;

#[repr(C)]
pub(crate) struct File {
    header: Header,
    ty: &'static Type,
    file: Option<std::fs::File>,
    writable: bool,
}

pub(crate) fn open(path: &str, mode: &str, ty: &'static Type) -> Result<u128, Error> {
    let writable = mode != "r";
    let mode = match mode {
        "r" => OpenMode::Read,
        "w" => OpenMode::Replace,
        "a" => OpenMode::Append,
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
    let file = unsafe { &mut (*pointer).file };
    let file = file.as_mut().ok_or(std::io::ErrorKind::NotConnected)?;
    Ok(text_io::read_all(file)? as u128)
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
    file.write_all(text.as_bytes())?;
    Ok(text.chars().count() as u128)
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
            });
            memory::plenty_retain(pointer.cast());
            memory::plenty_release(pointer.cast());
            assert!(closed(pointer));
            assert_eq!(close(pointer).unwrap(), 0);
            memory::plenty_release(pointer.cast());
        }
    }
}

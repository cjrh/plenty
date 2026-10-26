//! Raw transport uses existing owned list[u8] storage. No text decoding or
//! newline translation occurs except at the explicit decode_utf8 boundary.
use super::{Collection, OwnedValue, Type};
use crate::text_io::{self, Error, OpenMode};
use crate::{memory::AllocError, strings};
use std::io::{Read, Write};

fn new(ty: &'static Type, capacity: usize) -> Result<OwnedValue<'static>, Error> {
    Ok(OwnedValue {
        value: super::try_collection_new(ty, capacity)? as u128,
        ty,
    })
}

unsafe fn append(owner: &OwnedValue<'_>, bytes: &[u8]) -> Result<(), Error> {
    // SAFETY: owner exclusively owns this initialized list[u8].
    unsafe {
        let list = &mut *(owner.value as *mut Collection);
        for byte in bytes {
            list.try_insert(*byte as u128, 0)?;
        }
    }
    Ok(())
}

unsafe fn copy(bytes: &[u8], ty: &'static Type) -> Result<u128, Error> {
    unsafe {
        let owner = new(ty, bytes.len())?;
        append(&owner, bytes)?;
        Ok(owner.into_value())
    }
}

fn contiguous(list: &Collection) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(list.entries.len())
        .map_err(|_| AllocError::OutOfMemory)?;
    bytes.extend(list.entries.iter().map(|entry| entry.key as u8));
    Ok(bytes)
}

fn write(list: &Collection, writer: &mut impl Write) -> Result<u128, Error> {
    let mut chunk = [0; 8192];
    let mut entries = list.entries.iter();
    loop {
        let mut count = 0;
        for byte in &mut chunk {
            let Some(entry) = entries.next() else { break };
            *byte = entry.key as u8;
            count += 1;
        }
        if count == 0 {
            break;
        }
        writer.write_all(&chunk[..count])?;
    }
    Ok(list.entries.len() as u128)
}

unsafe fn record(reader: &mut impl Read, delimiter: u8, ty: &'static Type) -> Result<u128, Error> {
    unsafe {
        let owner = new(ty, 0)?;
        let mut byte = [0];
        loop {
            match reader.read(&mut byte) {
                Ok(0) if (*(owner.value as *const Collection)).entries.is_empty() => {
                    return Ok(super::wrap(0, 0));
                }
                Ok(0) => break,
                Ok(_) if byte[0] == delimiter => break,
                Ok(_) => append(&owner, &byte)?,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(super::wrap(owner.into_value(), 1))
    }
}

/// Operands are borrowed live values of the statically checked signature.
// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(super) unsafe fn call(
    op: i64,
    a: u128,
    b: u128,
    descriptor: *const Type,
) -> Result<u128, Error> {
    unsafe {
        match op {
            142 => {
                let ty = (*descriptor).variants[0].fields[0];
                let args = text_io::argument_bytes();
                let owner = new(ty, args.len())?;
                for arg in args {
                    let child = OwnedValue {
                        value: copy(arg, ty.key())?,
                        ty: ty.key(),
                    };
                    // Unlike text, a list is affine. Transfer the child only
                    // after storing succeeds; its guard cleans up on failure.
                    (*(owner.value as *mut Collection)).try_store(child.value, 0)?;
                    child.into_value();
                }
                Ok(owner.into_value())
            }
            143 => {
                let option = (*descriptor).variants[0].fields[0];
                record(&mut text_io::Stdin, a as u8, option.variants[1].fields[0])
            }
            144 => {
                let text = a as *const strings::Text;
                copy(strings::bytes(&text), (*descriptor).variants[0].fields[0])
            }
            145 => {
                let bytes = contiguous(&*(a as *const Collection))?;
                let text = std::str::from_utf8(&bytes).map_err(|_| Error::InvalidUtf8)?;
                Ok(strings::try_new(text)? as u128)
            }
            146 => write(&*(a as *const Collection), &mut std::io::stdout().lock()),
            147 => write(&*(a as *const Collection), &mut crate::io::stderr()),
            148 | 149 => {
                let path = contiguous(&*(a as *const Collection))?;
                let mut file = text_io::open_file_bytes(
                    &path,
                    if op == 148 {
                        OpenMode::Read
                    } else {
                        OpenMode::Replace
                    },
                )?;
                if op == 149 {
                    let count = write(&*(b as *const Collection), &mut file)?;
                    text_io::close(file)?;
                    return Ok(count);
                }
                let owner = new((*descriptor).variants[0].fields[0], 0)?;
                let mut chunk = [0; 8192];
                loop {
                    match file.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(count) => append(&owner, &chunk[..count])?,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error.into()),
                    }
                }
                text_io::close(file)?;
                Ok(owner.into_value())
            }
            _ => unreachable!("byte transport opcode"),
        }
    }
}

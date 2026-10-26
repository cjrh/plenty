//! Three inotify syscalls. File ownership, byte transport and readiness use
//! existing runtime facilities; bounded batches and event decoding are Plenty.
use super::Type;
use crate::text_io::{self, Error, Kind};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod native {
    use super::*;
    use crate::{files, memory::AllocError};
    use std::os::fd::FromRawFd;

    unsafe extern "C" {
        fn inotify_init1(flags: i32) -> i32;
        fn inotify_add_watch(fd: i32, path: *const std::ffi::c_char, mask: u32) -> i32;
        fn inotify_rm_watch(fd: i32, watch: i32) -> i32;
    }

    fn checked(value: i32) -> Result<i32, Error> {
        if value < 0 {
            Err(std::io::Error::last_os_error().into())
        } else {
            Ok(value)
        }
    }

    fn open(ty: &'static Type) -> Result<u128, Error> {
        // Allocate the owner before creating a descriptor. File closes it on
        // explicit close, drop, or any later failure constructing the facade.
        files::prepare(true, ty, || {
            const NONBLOCK: i32 = 0o4000;
            const CLOEXEC: i32 = 0o2000000;
            // SAFETY: flags are valid on the supported Linux ABI.
            let fd = checked(unsafe { inotify_init1(NONBLOCK | CLOEXEC) })?;
            // SAFETY: init1 returned a new descriptor owned only here.
            Ok(unsafe { std::fs::File::from_raw_fd(fd) })
        })
    }

    #[inline(always)]
    pub(super) unsafe fn call(op: i64, a: u128, b: u128, ty: &'static Type) -> Result<u128, Error> {
        if op == 512 {
            return open(ty.variants[0].fields[0]);
        }
        // SAFETY: compiler signatures supply live loans of File and list[u8].
        unsafe {
            let fd = files::descriptor(a as *const _)?;
            match op {
                513 => {
                    let mut path = super::super::filesystem::contiguous(b)?;
                    if path.contains(&0) {
                        return Err(Kind::InvalidInput.into());
                    }
                    path.try_reserve(1).map_err(|_| AllocError::OutOfMemory)?;
                    path.push(0);
                    // Content, metadata, close-after-write, child creation and
                    // deletion, moves and self deletion/move; not access/open.
                    const CHANGES: u32 = 0x0000_0fce;
                    Ok(checked(inotify_add_watch(fd, path.as_ptr().cast(), CHANGES))? as u128)
                }
                514 => {
                    checked(inotify_rm_watch(fd, b as i32))?;
                    Ok(0)
                }
                _ => crate::fail("invalid filesystem watch operation"),
            }
        }
    }
}

#[inline(always)]
pub(super) unsafe fn call(op: i64, a: u128, b: u128, descriptor: *const Type) -> u128 {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        // SAFETY: codegen supplies live borrowed operands and output metadata.
        text_io::result(unsafe { native::call(op, a, b, &*descriptor) })
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = (op, a, b, descriptor);
        text_io::result(Err(Kind::Unsupported.into()))
    }
}

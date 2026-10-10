//! Reports the error a program's `main` returned, on stderr.
use super::{render, Type};
use crate::render_buffer::Buffer;
use std::io::Write;

/// Written when the detailed message cannot be built. It needs no allocation.
const FALLBACK: &[u8] = b"error: main returned Err\n";

/// Best effort: a failed allocation or an unwritable stderr loses the message,
/// never the caller's cleanup or exit status. A null `ty` marks an error type
/// that has no rendering.
pub(super) unsafe fn report(value: u128, ty: *const Type) {
    crate::io::flush();
    let mut out = Buffer::default();
    // SAFETY: the entry wrapper passes the live error payload with its own
    // static metadata, and keeps ownership of the payload.
    if let Some(ty) = unsafe { ty.as_ref() } {
        out.extend_from_slice(b"error: main returned ");
        unsafe { render(value, ty, &mut out) };
        describe(value, ty, &mut out);
        out.push(b'\n');
    }
    let message = out.finish();
    let bytes = match &message {
        Ok(bytes) if !bytes.is_empty() => bytes.as_slice(),
        _ => FALLBACK,
    };
    // One write keeps the line whole. Its failure is not reported anywhere:
    // stderr is the only channel left.
    without_sigpipe(|| {
        let _ = std::io::stderr().lock().write_all(bytes);
    });
}

/// A reader that closed its end of stderr must not kill the process before
/// the caller's cleanup and exit status.
#[cfg(unix)]
fn without_sigpipe(write: impl FnOnce()) {
    unsafe extern "C" {
        fn signal(signal: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13;
    const SIG_IGN: usize = 1;
    // SAFETY: ignoring a signal installs no handler code, and the previous
    // disposition is restored unchanged.
    unsafe {
        let previous = signal(SIGPIPE, SIG_IGN);
        write();
        signal(SIGPIPE, previous);
    }
}

#[cfg(not(unix))]
fn without_sigpipe(write: impl FnOnce()) {
    write()
}

/// Explain the standard errors whose rendering does not say what went wrong.
fn describe(value: u128, ty: &Type, out: &mut Buffer) {
    if ty.kind != b'B' {
        return;
    }
    let description: &[&str] = match (ty.name, ty.variants[ty.tag(value)].name) {
        ("Failure", _) => &["Failure keeps no details of the original error"],
        ("IoError", "System") => return system_description(value as i32, out),
        ("IoError", "InvalidMode") => &["the open mode must be one of ", crate::open_modes::NAMES],
        ("IoError", "Closed") => &["the file is closed"],
        ("IoError", "NotReadable") => &["the file is not open for reading"],
        ("IoError", "NotWritable") => &["the file is not open for writing"],
        ("IoError", "InvalidInput") => &["an argument is outside the accepted range"],
        ("IoError", "Unsupported") => &["this platform does not support the operation"],
        ("IoError", "Other") => &["the operating system supplied no error code"],
        _ => return,
    };
    out.extend_from_slice(b": ");
    for part in description {
        out.extend_from_slice(part.as_bytes());
    }
}

/// Append the C library's text for an OS error code, using only stack storage.
#[cfg(unix)]
fn system_description(code: i32, out: &mut Buffer) {
    unsafe extern "C" {
        // The unprefixed Linux symbol may be the GNU variant, which returns a
        // pointer rather than filling the buffer.
        #[cfg_attr(target_os = "linux", link_name = "__xpg_strerror_r")]
        fn strerror_r(code: i32, buffer: *mut std::ffi::c_char, length: usize) -> i32;
    }
    let mut text = [0u8; 128];
    // SAFETY: the buffer is writable for the stated length; a successful call
    // leaves a NUL-terminated string in it.
    if unsafe { strerror_r(code, text.as_mut_ptr().cast(), text.len()) } != 0 {
        return;
    }
    let length = text.iter().position(|byte| *byte == 0).unwrap_or(0);
    if let Ok(text) = std::str::from_utf8(&text[..length]) {
        out.extend_from_slice(b": ");
        out.extend_from_slice(text.as_bytes());
    }
}

#[cfg(not(unix))]
fn system_description(_code: i32, _out: &mut Buffer) {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn os_codes_get_the_c_library_description_and_unknown_codes_get_none() {
        for (code, expected) in [(2, ": No such file or directory"), (-1, "")] {
            let mut out = Buffer::default();
            system_description(code, &mut out);
            assert_eq!(out.finish().unwrap(), expected.as_bytes());
        }
    }
}

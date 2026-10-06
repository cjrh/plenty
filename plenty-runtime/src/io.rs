use crate::strings::{self, Text};
use std::io::Write;

pub(crate) fn system_error(error: std::io::Error) -> u128 {
    crate::aggregates::wrap(error.raw_os_error().unwrap_or(0) as u32 as u128, 0)
}

pub(crate) fn io_result(result: std::io::Result<u128>) -> u128 {
    use crate::aggregates::wrap;
    match result {
        Ok(value) => wrap(value, 0),
        Err(error) => wrap(system_error(error), 1),
    }
}

pub(crate) unsafe fn write_stream(text: *const Text, stderr: bool) -> u128 {
    // SAFETY: the dispatcher borrows a live text throughout this call.
    let source = unsafe { strings::utf8(text) };
    let result = if stderr {
        std::io::stderr().lock().write_all(source.as_bytes())
    } else {
        std::io::stdout().lock().write_all(source.as_bytes())
    };
    io_result(result.map(|()| source.chars().count() as u128))
}

pub(crate) fn flush_stream(stderr: bool) -> u128 {
    let result = if stderr {
        std::io::stderr().lock().flush()
    } else {
        std::io::stdout().lock().flush()
    };
    io_result(result.map(|()| 0))
}

pub(crate) fn output(bytes: &[u8]) {
    if std::io::stdout().lock().write_all(bytes).is_err() {
        crate::fail("cannot write stdout");
    }
}
pub(crate) fn flush() {
    let _ = std::io::stdout().lock().flush();
}

pub(crate) unsafe fn repr(text: *const Text, legacy: bool, out: &mut Vec<u8>) {
    out.push(b'"');
    // SAFETY: the caller borrows a live, validated string throughout rendering.
    for &byte in unsafe { strings::bytes(text) } {
        match byte {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0 => out.extend_from_slice(b"\\0"),
            c if c < 0x20 || (legacy && c > 0x7e) => {
                write!(out, "\\u{{{c:x}}}").unwrap();
            }
            c => out.push(c),
        }
    }
    out.push(b'"');
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_print_str(text: *const Text) {
    let mut out = Vec::new();
    unsafe {
        repr(text, true, &mut out);
    }
    output(&out);
}
#[no_mangle]
pub(crate) unsafe extern "C" fn plenty_println(text: *const Text) {
    #[cfg(feature = "allocation-checks")]
    if unsafe { strings::bytes(text) } == b"__test_small_live_heap__" {
        crate::accounting::checkpoint();
    }
    #[cfg(feature = "allocation-checks")]
    match unsafe { strings::bytes(text) } {
        b"__test_begin_no_allocations__" => crate::accounting::begin_no_allocations(),
        b"__test_end_no_allocations__" => crate::accounting::end_no_allocations(),
        b"__test_restore_allocations__" => crate::accounting::fail_after(None),
        marker if marker.starts_with(b"__test_fail_allocations_after_") => {
            let count = marker
                .strip_prefix(b"__test_fail_allocations_after_")
                .and_then(|n| n.strip_suffix(b"__"))
                .and_then(|n| std::str::from_utf8(n).ok())
                .and_then(|n| n.parse().ok())
                .expect("valid allocation failure marker");
            crate::accounting::fail_after(Some(count));
        }
        _ => {}
    }
    unsafe {
        output(strings::bytes(text));
    }
    output(b"\n");
}
macro_rules! printer {
    ($name:ident, $ty:ty, $format:literal) => {
        #[no_mangle]
        pub(crate) extern "C" fn $name(value: $ty) {
            output(format!($format, value).as_bytes());
        }
    };
}
printer!(plenty_println_signed, i64, "{}\n");
printer!(plenty_println_unsigned, u64, "{}\n");
printer!(plenty_println_f32, f32, "{:?}\n");
printer!(plenty_println_f64, f64, "{:?}\n");
printer!(plenty_print_i8, i8, "{}i8");
printer!(plenty_print_i16, i16, "{}i16");
printer!(plenty_print_i32, i32, "{}i32");
printer!(plenty_print_i64, i64, "{}i64");
printer!(plenty_print_u8, u8, "{}u8");
printer!(plenty_print_u16, u16, "{}u16");
printer!(plenty_print_u32, u32, "{}u32");
printer!(plenty_print_u64, u64, "{}u64");
#[no_mangle]
pub(crate) extern "C" fn plenty_println_bool(value: i8) {
    output(if value != 0 { b"True\n" } else { b"False\n" });
}
#[no_mangle]
pub(crate) extern "C" fn plenty_print_bool(value: i8) {
    output(if value != 0 { b"true" } else { b"false" });
}
#[no_mangle]
pub(crate) extern "C" fn plenty_print_open_bracket() {
    output(b"[");
}
#[no_mangle]
pub(crate) extern "C" fn plenty_print_close_bracket() {
    output(b"]\n");
}
#[no_mangle]
pub(crate) extern "C" fn plenty_print_space() {
    output(b" ");
}
#[no_mangle]
pub(crate) extern "C" fn plenty_trap_overflow() -> ! {
    crate::fail("integer overflow")
}
#[no_mangle]
pub(crate) extern "C" fn plenty_trap_div_zero() -> ! {
    crate::fail("division by zero")
}

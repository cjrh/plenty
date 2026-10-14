//! Native back end of `std.compress`: DEFLATE, zlib and gzip streams from the
//! zlib-rs crate. Nothing here implements compression or checksums; buffering,
//! limits and the trailing-data policy live in `std.compress`.
//!
//! A codec lives in a runtime object whose prefix is byte storage, so
//! `std.compress` holds it as an ordinary `Bytes`. That buffer is the codec's
//! pending output: its capacity is reserved once, at creation, and a step
//! writes only into the reserved space, so stepping never allocates.
//! `std.compress` drains it with ordinary byte operations. Every operation
//! checks the object's destructor and stops the process on any other buffer.
//!
//! zlib-rs is driven through its zlib-style calls (`__internal-api`), whose
//! `init` reports allocation failure as `MemError` instead of aborting.
use super::byte_storage::{range, result, Storage};
use crate::memory::{self, AllocError, Header};
use std::ffi::CStr;
use std::ptr;
use zlib_rs::c_api::z_stream;
use zlib_rs::deflate::{self, DeflateConfig, DeflateStream};
use zlib_rs::inflate::{self, InflateConfig, InflateStream};
use zlib_rs::{DeflateFlush, InflateFlush, ReturnCode};

/// Step results shared with `std.compress`; nonnegative values are counts.
const CORRUPT: i64 = -1;
const CHECKSUM: i64 = -2;
const OUT_OF_MEMORY: i64 = -3;
const FINISHED: i64 = -4;

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Encoder,
    Decoder,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// `stream` has not been initialised; only destruction may follow.
    Unstarted,
    Live,
    /// A decoder reached the end of its stream, or an encoder finished it.
    Ended,
}

#[repr(C)]
struct State {
    storage: Storage,
    /// zlib-rs keeps a pointer back to its `z_stream`, so the stream is
    /// initialised in place in this heap object and never moves.
    stream: z_stream,
    role: Role,
    phase: Phase,
    /// The output reserved at creation; a step never writes past it.
    capacity: usize,
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    // SAFETY: the last owner has gone; this runs once, and `pointer` was
    // allocated by `create` with `State`'s layout.
    unsafe {
        let state = pointer.cast::<State>();
        release(state);
        ptr::drop_in_place(ptr::addr_of_mut!((*state).storage.bytes));
        memory::free::<State, u8>(state, 0);
    }
}

/// Frees the codec's own allocations.
unsafe fn release(state: *mut State) {
    // SAFETY: `state` is live; `from_stream_mut` rejects an uninitialised stream.
    unsafe {
        if (*state).phase == Phase::Unstarted {
            return;
        }
        let stream = ptr::addr_of_mut!((*state).stream);
        match (*state).role {
            Role::Encoder => {
                if let Some(stream) = DeflateStream::from_stream_mut(stream) {
                    // `Err` only reports that the stream was mid-way; the
                    // memory is released either way.
                    let _ = deflate::end(stream);
                }
            }
            Role::Decoder => {
                if let Some(stream) = InflateStream::from_stream_mut(stream) {
                    inflate::end(stream);
                }
            }
        }
        (*state).phase = Phase::Unstarted;
    }
}

/// Kinds 1-3 encode and 4-6 decode raw DEFLATE, zlib and gzip in that order;
/// `std.compress` has validated the level and the capacity.
fn create(kind: u128, level: i64, capacity: usize) -> Result<u128, AllocError> {
    let (role, format) = match kind {
        1..=3 => (Role::Encoder, kind),
        4..=6 => (Role::Decoder, kind - 3),
        _ => crate::fail("unknown compression format"),
    };
    // zlib's window-bits convention selects the framing: negative for raw
    // DEFLATE, 8-15 for zlib, 16 more for gzip. 15 is the 32 KiB window.
    let window_bits = match format {
        1 => -15,
        2 => 15,
        _ => 31,
    };
    let mut bytes = Vec::new();
    memory::try_reserve(&mut bytes, capacity)?;
    let owner = memory::try_allocate::<State, u8>(0)?;
    // SAFETY: fresh, exclusively owned memory with `State`'s layout; once
    // written, `destroy` releases whatever was obtained.
    unsafe {
        owner.write(State {
            storage: Storage {
                header: Header::new(destroy),
                bytes,
            },
            stream: z_stream::default(),
            role,
            phase: Phase::Unstarted,
            capacity,
        });
        let stream = &mut (*owner).stream;
        let code = match role {
            Role::Encoder => deflate::init(
                stream,
                DeflateConfig {
                    level: level as i32,
                    window_bits,
                    ..DeflateConfig::default()
                },
            ),
            Role::Decoder => inflate::init(stream, InflateConfig { window_bits }),
        };
        match code {
            ReturnCode::Ok => (*owner).phase = Phase::Live,
            ReturnCode::MemError => {
                destroy(owner.cast());
                return Err(AllocError::OutOfMemory);
            }
            _ => crate::fail("zlib-rs rejected its configuration"),
        }
    }
    Ok(owner as u128)
}

/// The state of a live codec object, or a stopped process for any other.
unsafe fn state(value: u128) -> *mut State {
    let state = value as *mut State;
    // SAFETY: every byte-storage-typed operand starts with a live `Header`.
    let owner = unsafe { (*state.cast::<Header>()).destroy };
    let ours: unsafe extern "C" fn(*mut Header) = destroy;
    if owner.map(|f| f as usize) != Some(ours as usize) {
        crate::fail("operand is not a std.compress state");
    }
    state
}

/// One codec call over `input` into the reserved output. Returns the bytes
/// consumed and zlib-rs's code.
unsafe fn run(state: *mut State, input: &[u8], flush: i32) -> (usize, ReturnCode) {
    // SAFETY: `state` is a live, started codec; `input` lies in another
    // buffer; the output pointer covers reserved, unused capacity only.
    unsafe {
        let bytes = &mut (*state).storage.bytes;
        let filled = bytes.len();
        let room = (*state)
            .capacity
            .min(bytes.capacity())
            .saturating_sub(filled)
            .min(u32::MAX as usize);
        let offered = input.len().min(u32::MAX as usize);
        let stream = &mut (*state).stream;
        stream.next_in = input.as_ptr();
        stream.avail_in = offered as u32;
        stream.next_out = bytes.as_mut_ptr().add(filled);
        stream.avail_out = room as u32;
        let code = match (*state).role {
            Role::Encoder => {
                let flush = match flush {
                    2 => DeflateFlush::SyncFlush,
                    4 => DeflateFlush::Finish,
                    _ => DeflateFlush::NoFlush,
                };
                match DeflateStream::from_stream_mut(stream) {
                    Some(stream) => deflate::deflate(stream, flush),
                    None => crate::fail("std.compress encoder is not initialised"),
                }
            }
            Role::Decoder => match InflateStream::from_stream_mut(stream) {
                Some(stream) => inflate::inflate(stream, InflateFlush::NoFlush),
                None => crate::fail("std.compress decoder is not initialised"),
            },
        };
        let stream = &mut (*state).stream;
        let produced = room - stream.avail_out as usize;
        let consumed = offered - stream.avail_in as usize;
        // No pointer into either buffer survives the call.
        stream.next_in = ptr::null();
        stream.next_out = ptr::null_mut();
        stream.avail_in = 0;
        stream.avail_out = 0;
        bytes.set_len(filled + produced);
        (consumed, code)
    }
}

/// A failure code, distinguishing integrity-check mismatches by zlib-rs's
/// message, the only place it records which check failed.
unsafe fn failure(state: *mut State, code: ReturnCode) -> i64 {
    match code {
        ReturnCode::MemError => OUT_OF_MEMORY,
        ReturnCode::DataError => {
            // SAFETY: zlib-rs sets `msg` to null or a static C string.
            let message = unsafe { (*state).stream.msg };
            if message.is_null() {
                return CORRUPT;
            }
            match unsafe { CStr::from_ptr(message) }.to_bytes() {
                b"incorrect data check" | b"incorrect length check" | b"header crc mismatch" => {
                    CHECKSUM
                }
                _ => CORRUPT,
            }
        }
        // A preset dictionary is unsupported, so a stream needing one is corrupt.
        _ => CORRUPT,
    }
}

/// Consumes from `input` until it is used up, the output is full, or a stream
/// ends. Returns the count consumed or a negative code.
unsafe fn step(value: u128, input: &[u8]) -> i64 {
    // SAFETY: forwarded from the caller.
    unsafe {
        let state = state(value);
        if (*state).phase != Phase::Live {
            return FINISHED;
        }
        let (consumed, code) = run(state, input, 0);
        match code {
            ReturnCode::Ok | ReturnCode::BufError => consumed as i64,
            ReturnCode::StreamEnd => {
                (*state).phase = Phase::Ended;
                consumed as i64
            }
            other => failure(state, other),
        }
    }
}

/// Mode 2 is a sync flush, 4 finishes the stream. 1 when complete, 0 when the
/// output filled first, or a negative code.
unsafe fn flush(value: u128, mode: i64) -> i64 {
    // SAFETY: forwarded from the caller.
    unsafe {
        let state = state(value);
        if (*state).role != Role::Encoder {
            crate::fail("std.compress decoders do not flush");
        }
        if (*state).phase != Phase::Live {
            return FINISHED;
        }
        let bytes = &(*state).storage.bytes;
        if bytes.len() >= (*state).capacity.min(bytes.capacity()) {
            return 0;
        }
        let (_, code) = run(state, &[], mode as i32);
        let full = (*state).stream.avail_out == 0 && {
            let bytes = &(*state).storage.bytes;
            bytes.len() >= (*state).capacity.min(bytes.capacity())
        };
        match code {
            ReturnCode::StreamEnd => {
                (*state).phase = Phase::Ended;
                1
            }
            // zlib's contract: a call that fills the output must be repeated.
            ReturnCode::Ok if full => 0,
            ReturnCode::Ok if mode == 4 => 0,
            // `BufError` means there was nothing left to flush.
            ReturnCode::Ok | ReturnCode::BufError => 1,
            other => failure(state, other),
        }
    }
}

/// Starts a decoder on a further stream, keeping its window allocation.
unsafe fn restart(value: u128) -> bool {
    // SAFETY: forwarded from the caller.
    unsafe {
        let state = state(value);
        if (*state).role != Role::Decoder || (*state).phase == Phase::Unstarted {
            return false;
        }
        let stream = ptr::addr_of_mut!((*state).stream);
        let Some(stream) = InflateStream::from_stream_mut(stream) else {
            return false;
        };
        if inflate::reset(stream) != ReturnCode::Ok {
            return false;
        }
        (*state).phase = Phase::Live;
        true
    }
}

/// Operands are live values of the statically checked signature; `std.bytes`
/// has validated each range, and the runtime repeats the check.
pub(super) unsafe fn call(op: i64, operands: [u128; 4]) -> u128 {
    let [a, b, c, d] = operands;
    // SAFETY: see above. The input view borrows a separate buffer's storage.
    unsafe {
        let slice = |value: u128, start: u128, stop: u128| {
            let bytes = (*(value as *const Storage)).bytes.as_slice();
            &bytes[range(start, stop, bytes.len())]
        };
        match op {
            320 => {
                let capacity = usize::try_from(c as i64)
                    .unwrap_or_else(|_| crate::fail("negative compression buffer"));
                result(create(a, b as i64, capacity))
            }
            321 => step(a, slice(b, c, d)) as u128,
            322 => flush(a, b as i64) as u128,
            323 => ((*state(a)).phase == Phase::Ended) as u128,
            324 => restart(a) as u128,
            325 => zlib_rs::crc32::crc32(a as u32, slice(b, c, d)) as u128,
            _ => crate::fail("unknown compression operation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn output(value: u128) -> Vec<u8> {
        unsafe { std::mem::take(&mut (*(value as *mut Storage)).bytes) }
    }

    unsafe fn drain(value: u128) -> Vec<u8> {
        unsafe {
            let bytes = &mut (*(value as *mut Storage)).bytes;
            let out = bytes.clone();
            bytes.clear();
            out
        }
    }

    #[test]
    fn small_buffers_round_trip_every_format() {
        let data: Vec<u8> = (0..20_000u32).map(|i| (i * i % 251) as u8).collect();
        for format in 1..=3u128 {
            let encoder = create(format, 6, 7).unwrap();
            let decoder = create(format + 3, 0, 5).unwrap();
            unsafe {
                let mut packed = Vec::new();
                let mut at = 0;
                while at < data.len() {
                    let used = step(encoder, &data[at..]);
                    assert!(used >= 0);
                    at += used as usize;
                    packed.extend(drain(encoder));
                }
                loop {
                    let done = flush(encoder, 4);
                    packed.extend(drain(encoder));
                    if done == 1 {
                        break;
                    }
                    assert_eq!(done, 0);
                }
                assert_eq!(step(encoder, b"x"), FINISHED);
                let mut plain = Vec::new();
                let mut at = 0;
                while (*state(decoder)).phase == Phase::Live {
                    let used = step(decoder, &packed[at..]);
                    assert!(used >= 0, "{used}");
                    at += used as usize;
                    plain.extend(drain(decoder));
                }
                assert_eq!(at, packed.len());
                assert_eq!(plain, data);
                let _ = output(encoder);
                destroy(encoder as *mut Header);
                destroy(decoder as *mut Header);
            }
        }
    }

    #[test]
    fn checksum_and_data_errors_are_distinguished() {
        let encoder = create(3, 9, 256).unwrap();
        unsafe {
            assert_eq!(step(encoder, b"hello hello hello"), 17);
            assert_eq!(flush(encoder, 4), 1);
            let packed = output(encoder);
            destroy(encoder as *mut Header);
            let decode = |data: &[u8]| {
                let decoder = create(6, 0, 256).unwrap();
                let code = step(decoder, data);
                destroy(decoder as *mut Header);
                code
            };
            assert_eq!(decode(&packed), packed.len() as i64);
            let mut bad_crc = packed.clone();
            let at = bad_crc.len() - 8;
            bad_crc[at] ^= 1;
            assert_eq!(decode(&bad_crc), CHECKSUM);
            let mut bad_length = packed.clone();
            let at = bad_length.len() - 1;
            bad_length[at] ^= 1;
            assert_eq!(decode(&bad_length), CHECKSUM);
            let mut bad_magic = packed;
            bad_magic[0] = 0;
            assert_eq!(decode(&bad_magic), CORRUPT);
        }
    }
}

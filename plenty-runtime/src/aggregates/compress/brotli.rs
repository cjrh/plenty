//! RFC 7932 through the pinned reference codec's fallible C allocator API.
//! No C-owned buffer escapes a call; pending output uses the same Storage
//! prefix and drain protocol as the other std.compress codecs.
use super::{CORRUPT, FINISHED, OUT_OF_MEMORY};
use crate::aggregates::byte_storage::Storage;
use crate::memory::{self, AllocError, Header};
use std::ffi::{c_int, c_void};
use std::ptr;

type Allocate = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void, *mut c_void);

#[link(name = "plenty_brotli", kind = "static")]
unsafe extern "C" {
    fn BrotliEncoderCreateInstance(
        allocate: Allocate,
        free: Free,
        opaque: *mut c_void,
    ) -> *mut c_void;
    fn BrotliEncoderDestroyInstance(state: *mut c_void);
    fn BrotliEncoderSetParameter(state: *mut c_void, parameter: c_int, value: u32) -> c_int;
    fn BrotliEncoderCompressStream(
        state: *mut c_void,
        operation: c_int,
        available_in: *mut usize,
        next_in: *mut *const u8,
        available_out: *mut usize,
        next_out: *mut *mut u8,
        total_out: *mut usize,
    ) -> c_int;
    fn BrotliEncoderIsFinished(state: *const c_void) -> c_int;
    fn BrotliEncoderHasMoreOutput(state: *const c_void) -> c_int;
    fn BrotliDecoderCreateInstance(
        allocate: Allocate,
        free: Free,
        opaque: *mut c_void,
    ) -> *mut c_void;
    fn BrotliDecoderDestroyInstance(state: *mut c_void);
    fn BrotliDecoderDecompressStream(
        state: *mut c_void,
        available_in: *mut usize,
        next_in: *mut *const u8,
        available_out: *mut usize,
        next_out: *mut *mut u8,
        total_out: *mut usize,
    ) -> c_int;
    fn BrotliDecoderGetErrorCode(state: *const c_void) -> c_int;
}

// C's malloc contract requires fundamental-type alignment. Brotli does not
// request over-aligned allocations; the prefix keeps the original size so
// free can use exactly the same Rust Layout, with no side allocation.
#[repr(C, align(16))]
struct Allocation {
    size: usize,
}

unsafe extern "C" fn allocate(opaque: *mut c_void, size: usize) -> *mut c_void {
    match memory::try_allocate::<Allocation, u8>(size) {
        Ok(pointer) => {
            // SAFETY: fresh allocation has room for the prefix and size bytes.
            unsafe {
                pointer.write(Allocation { size });
                pointer.add(1).cast()
            }
        }
        Err(_) => {
            // SAFETY: opaque points to the live state's stable failure flag.
            unsafe { *opaque.cast::<bool>() = true };
            ptr::null_mut()
        }
    }
}

unsafe extern "C" fn free(_: *mut c_void, address: *mut c_void) {
    if address.is_null() {
        return;
    }
    // SAFETY: the codec returns each successful callback allocation exactly
    // once, including its partially initialised and failed-stream allocations.
    unsafe {
        let prefix = address.cast::<Allocation>().sub(1);
        memory::free::<Allocation, u8>(prefix, (*prefix).size);
    }
}

#[repr(C)]
struct State {
    storage: Storage,
    codec: *mut c_void,
    encoder: bool,
    ended: bool,
    oom: bool,
    failure: i64,
    capacity: usize,
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    // SAFETY: the unique final owner has the State layout. The C destructor
    // frees its allocations through our callback before State itself is freed.
    unsafe {
        let state = pointer.cast::<State>();
        if (*state).encoder {
            BrotliEncoderDestroyInstance((*state).codec);
        } else {
            BrotliDecoderDestroyInstance((*state).codec);
        }
        ptr::drop_in_place(ptr::addr_of_mut!((*state).storage.bytes));
        memory::free::<State, u8>(state, 0);
    }
}

pub(super) unsafe fn owns(value: u128) -> bool {
    // SAFETY: compress operands are live byte-storage objects with Header.
    let destructor = unsafe { (*(value as *const Header)).destroy };
    destructor.map(|function| function as usize) == Some(destroy as *const () as usize)
}

pub(super) fn create(encoder: bool, quality: i64, capacity: usize) -> Result<u128, AllocError> {
    if encoder && !(0..=11).contains(&quality) {
        crate::fail("invalid Brotli quality");
    }
    let mut bytes = Vec::new();
    memory::try_reserve(&mut bytes, capacity)?;
    let owner = memory::try_allocate::<State, u8>(0)?;
    // SAFETY: write a complete state before any C callback can access oom.
    // Neither this state nor its flag moves while the codec exists.
    unsafe {
        owner.write(State {
            storage: Storage {
                header: Header::new(destroy),
                bytes,
            },
            codec: ptr::null_mut(),
            encoder,
            ended: false,
            oom: false,
            failure: 0,
            capacity,
        });
        let opaque = ptr::addr_of_mut!((*owner).oom).cast();
        (*owner).codec = if encoder {
            BrotliEncoderCreateInstance(allocate, free, opaque)
        } else {
            BrotliDecoderCreateInstance(allocate, free, opaque)
        };
        if (*owner).codec.is_null() {
            destroy(owner.cast());
            return Err(AllocError::OutOfMemory);
        }
        if encoder {
            // RFC window 22 is the reference default. No large-window mode or
            // custom dictionaries; callers control quality only.
            let valid = BrotliEncoderSetParameter((*owner).codec, 1, quality as u32) != 0
                && BrotliEncoderSetParameter((*owner).codec, 2, 22) != 0;
            if !valid {
                destroy(owner.cast());
                crate::fail("Brotli rejected its configuration");
            }
        }
    }
    Ok(owner as u128)
}

unsafe fn run(state: *mut State, input: &[u8], operation: c_int) -> i64 {
    // SAFETY: caller verified this object's identity. C borrows input/output
    // for this call only; pointers and lengths are disjoint from the oom flag
    // its allocation callback may update. Output room is reserved and writable.
    unsafe {
        if (*state).failure != 0 {
            return (*state).failure;
        }
        if (*state).ended {
            return FINISHED;
        }
        let bytes = &mut (*state).storage.bytes;
        let filled = bytes.len();
        let room = (*state)
            .capacity
            .min(bytes.capacity())
            .saturating_sub(filled);
        let mut available_in = input.len();
        let mut next_in = input.as_ptr();
        let mut available_out = room;
        let mut next_out = bytes.as_mut_ptr().add(filled);
        let result = if (*state).encoder {
            BrotliEncoderCompressStream(
                (*state).codec,
                operation,
                &mut available_in,
                &mut next_in,
                &mut available_out,
                &mut next_out,
                ptr::null_mut(),
            )
        } else {
            BrotliDecoderDecompressStream(
                (*state).codec,
                &mut available_in,
                &mut next_in,
                &mut available_out,
                &mut next_out,
                ptr::null_mut(),
            )
        };
        bytes.set_len(filled + room - available_out);
        if result == 0 {
            let decoder_oom = !(*state).encoder
                && (-30..=-21).contains(&BrotliDecoderGetErrorCode((*state).codec));
            (*state).failure = if (*state).oom || decoder_oom {
                OUT_OF_MEMORY
            } else {
                CORRUPT
            };
            return (*state).failure;
        }
        (*state).ended = if (*state).encoder {
            BrotliEncoderIsFinished((*state).codec) != 0
        } else {
            result == 1
        };
        (input.len() - available_in) as i64
    }
}

pub(super) unsafe fn step(value: u128, input: &[u8]) -> i64 {
    // SAFETY: caller checked owns(value).
    unsafe { run(value as *mut State, input, 0) }
}

pub(super) unsafe fn flush(value: u128, mode: i64) -> i64 {
    // SAFETY: caller checked owns(value); C state is live until destruction.
    unsafe {
        let state = value as *mut State;
        if !(*state).encoder {
            crate::fail("Brotli decoders do not flush");
        }
        let operation = match mode {
            2 => 1,
            4 => 2,
            _ => crate::fail("unknown Brotli flush mode"),
        };
        let result = run(state, &[], operation);
        if result < 0 {
            return result;
        }
        if (*state).ended || (mode == 2 && BrotliEncoderHasMoreOutput((*state).codec) == 0) {
            1
        } else {
            0
        }
    }
}

pub(super) unsafe fn ended(value: u128) -> bool {
    // SAFETY: caller checked owns(value).
    unsafe { (*(value as *const State)).ended }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn drain(value: u128, output: &mut Vec<u8>) {
        // SAFETY: tests retain a live unique State until destroy below.
        unsafe {
            let bytes = &mut (*(value as *mut State)).storage.bytes;
            output.extend_from_slice(bytes);
            bytes.clear();
        }
    }

    #[test]
    fn tiny_windows_all_qualities_and_empty_input() {
        let data: Vec<u8> = (0..40_000u32)
            .map(|i| ((i * i + i / 17) % 251) as u8)
            .collect();
        for quality in 0..=11 {
            for input in [&b""[..], &data[..]] {
                let encoder = create(true, quality, 7).unwrap();
                let decoder = create(false, 0, 5).unwrap();
                let mut packed = Vec::new();
                let mut plain = Vec::new();
                unsafe {
                    let mut at = 0;
                    while at < input.len() {
                        let used = step(encoder, &input[at..]);
                        assert!(used >= 0);
                        at += used as usize;
                        drain(encoder, &mut packed);
                    }
                    loop {
                        let done = flush(encoder, 4);
                        drain(encoder, &mut packed);
                        if done == 1 {
                            break;
                        }
                        assert_eq!(done, 0);
                    }
                    assert_eq!(step(encoder, b"x"), FINISHED);
                    let mut at = 0;
                    let mut rounds = 0;
                    while !ended(decoder) {
                        let used = step(decoder, &packed[at..]);
                        assert!(used >= 0);
                        at += used as usize;
                        drain(decoder, &mut plain);
                        rounds += 1;
                        assert!(rounds < 20_000);
                    }
                    assert_eq!(at, packed.len());
                    assert_eq!(plain, input);
                    destroy(encoder as *mut Header);
                    destroy(decoder as *mut Header);
                }
            }
        }
    }

    #[cfg(feature = "allocation-checks")]
    #[test]
    fn every_encoder_allocation_can_fail_without_panicking() {
        let input = b"a Brotli allocation failure probe ".repeat(256);
        for quality in [0, 5, 11] {
            let mut succeeded = false;
            for budget in 0..512 {
                crate::accounting::fail_after(Some(budget));
                let mut result = OUT_OF_MEMORY;
                if let Ok(encoder) = create(true, quality, 16384) {
                    unsafe {
                        result = step(encoder, &input);
                        if result >= 0 {
                            (*(encoder as *mut State)).storage.bytes.clear();
                            loop {
                                result = flush(encoder, 4);
                                (*(encoder as *mut State)).storage.bytes.clear();
                                if result != 0 {
                                    break;
                                }
                            }
                        }
                        destroy(encoder as *mut Header);
                    }
                }
                crate::accounting::fail_after(None);
                assert!(
                    result == 1 || result == OUT_OF_MEMORY,
                    "quality={quality} budget={budget} result={result}"
                );
                if result == 1 {
                    eprintln!("Brotli quality {quality}: allocation budgets 0..={budget} verified");
                    succeeded = true;
                    break;
                }
            }
            assert!(succeeded, "quality {quality} never completed");
        }
    }

    #[cfg(feature = "allocation-checks")]
    #[test]
    fn every_decoder_allocation_can_fail_without_panicking() {
        let input: Vec<u8> = (0..40_000u32)
            .map(|i| ((i * i + i / 17) % 251) as u8)
            .collect();
        let encoder = create(true, 11, 65536).unwrap();
        let mut packed = Vec::new();
        unsafe {
            assert_eq!(step(encoder, &input), input.len() as i64);
            drain(encoder, &mut packed);
            assert_eq!(flush(encoder, 4), 1);
            drain(encoder, &mut packed);
            destroy(encoder as *mut Header);
        }
        let mut succeeded = false;
        for budget in 0..128 {
            crate::accounting::fail_after(Some(budget));
            let mut result = OUT_OF_MEMORY;
            let mut at = 0;
            let mut produced = 0;
            if let Ok(decoder) = create(false, 0, 1024) {
                unsafe {
                    loop {
                        result = step(decoder, &packed[at..]);
                        if result < 0 {
                            break;
                        }
                        at += result as usize;
                        let bytes = &mut (*(decoder as *mut State)).storage.bytes;
                        produced += bytes.len();
                        bytes.clear();
                        if ended(decoder) {
                            result = 1;
                            break;
                        }
                    }
                    destroy(decoder as *mut Header);
                }
            }
            crate::accounting::fail_after(None);
            assert!(
                result == 1 || result == OUT_OF_MEMORY,
                "budget={budget} result={result}"
            );
            if result == 1 {
                assert_eq!(at, packed.len());
                assert_eq!(produced, input.len());
                eprintln!("Brotli decoder: allocation budgets 0..={budget} verified");
                succeeded = true;
                break;
            }
        }
        assert!(succeeded);
    }
}

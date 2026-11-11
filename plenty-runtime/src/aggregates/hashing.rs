//! Key the complete structural input, rather than an unkeyed prehash whose
//! collisions would survive any finalizer or secret suffix.
//!
//! SipHash-1-3 is the schedule Rust's `HashMap`, CPython and Swift use for
//! keyed hash tables. Bucket hashes are never revealed to programs, so an
//! attacker must find collisions without the per-process key; no published
//! attack on SipHash-1-3 does that.
use super::{Text, Type, strings};
use siphasher::sip::SipHasher13;
use std::hash::Hasher;
use std::io;
use std::sync::OnceLock;

static KEY: OnceLock<[u8; 16]> = OnceLock::new();

fn read_key(mut read: impl FnMut(&mut [u8]) -> io::Result<u128>) -> io::Result<[u8; 16]> {
    let mut key = [0; 16];
    let mut filled = 0;
    while filled < key.len() {
        match read(&mut key[filled..]) {
            Ok(count) if count > 0 && count <= (key.len() - filled) as u128 => {
                filled += count as usize;
            }
            Ok(_) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(key)
}

/// Dispatches in the caller, which has the key's kind at hand, so a text
/// key's hash keeps no frame for the framed encoding of other kinds
/// (issue #931).
#[inline]
pub(super) unsafe fn hash(value: u128, ty: &Type) -> u64 {
    if matches!(ty.kind, b's' | b'y') {
        // SAFETY: the caller supplies a live str or bytes key.
        unsafe { hash_text(value as *const Text) }
    } else {
        // SAFETY: forwarded from the typed collection operation.
        unsafe { hash_other(value, ty) }
    }
}

fn key() -> &'static [u8; 16] {
    // Creating or reserving an empty collection does not require entropy.
    // OnceLock publishes one complete key to all workers; no allocation or
    // user code is involved. Never substitute predictable bytes on OS failure.
    KEY.get_or_init(|| {
        read_key(crate::entropy::read)
            .unwrap_or_else(|_| crate::fail("collection hashing requires OS entropy"))
    })
}

/// The hash of an inline text, computed in the caller; `None` before the
/// first hash of the process has read the key.
#[inline(always)]
pub(super) fn inline_text(word: u64) -> Option<u64> {
    debug_assert!(strings::is_inline(word as usize as *const Text));
    // SAFETY: an inline text is its word; nothing is dereferenced.
    KEY.get()
        .map(|key| unsafe { text_with_key(word as usize as *const Text, key) })
}

/// The hash of a heap or literal text, computed in the caller; `None`
/// before the first hash of the process has read the key.
#[inline(always)]
pub(super) unsafe fn heap_text(text: *const Text) -> Option<u64> {
    // SAFETY: the caller passes a live text that is not inline.
    KEY.get()
        .map(|key| sip13_bytes(key, unsafe { strings::heap_bytes(text) }))
}

/// The hash of the str or bytes key holding `bytes`, before that key
/// exists (issue #949); `None` before the first hash of the process has read
/// the key. Equal to the built key's hash in either form.
#[inline(always)]
pub(super) fn text_bytes(bytes: &[u8]) -> Option<u64> {
    KEY.get().map(|key| sip13_bytes(key, bytes))
}

/// `text_bytes`, reading the process key first when no hash has yet.
#[inline(never)]
pub(super) fn hash_text_bytes(bytes: &[u8]) -> u64 {
    sip13_bytes(key(), bytes)
}

#[inline(never)]
pub(super) unsafe fn hash_text(text: *const Text) -> u64 {
    // SAFETY: forwarded from `hash`.
    unsafe { text_with_key(text, key()) }
}

#[inline(never)]
unsafe fn hash_other(value: u128, ty: &Type) -> u64 {
    // SAFETY: forwarded from `hash`.
    unsafe { hash_with_key(value, ty, key()) }
}

/// SipHash-1-3 of a str or bytes value's bytes. Inline and heap texts with
/// equal contents hash alike.
#[inline(always)]
unsafe fn text_with_key(text: *const Text, key: &[u8; 16]) -> u64 {
    if strings::is_inline(text) {
        // At most seven bytes: no whole block, and the bytes are already
        // the low part of the final one (issue #931).
        let word = text as usize as u64;
        let len = (word & 0xff) >> 1;
        let bytes = (word >> 8) & !(u64::MAX << (8 * len));
        sip13_finish(sip13_start(key), bytes | len << 56)
    } else {
        // SAFETY: a str/bytes key that is not inline is a live Text.
        sip13_bytes(key, unsafe { strings::heap_bytes(text) })
    }
}

pub(super) unsafe fn hash_with_key(value: u128, ty: &Type, key: &[u8; 16]) -> u64 {
    // Every key in one collection has the same descriptor, so a top-level key
    // needs no kind byte. SipHash pads its one message injectively (the final
    // block holds the length), so a lone string needs no length prefix either.
    // Keys nested in tuples, classes and enums keep the framed encoding below.
    match ty.kind {
        // SAFETY: a str/bytes key is a live Text or an inline word.
        b's' | b'y' => unsafe { text_with_key(value as *const Text, key) },
        b'1'..=b'8' | b'b' => sip13_word(key, scalar(value, ty.kind)),
        _ => {
            let mut state = SipHasher13::new_with_key(key);
            // SAFETY: forwarded from the typed collection operation.
            unsafe { write_key(&mut state, value, ty) };
            state.finish()
        }
    }
}

#[inline(always)]
fn sip13_start(key: &[u8; 16]) -> [u64; 4] {
    let k0 = u64::from_le_bytes(key[..8].try_into().unwrap());
    let k1 = u64::from_le_bytes(key[8..].try_into().unwrap());
    [
        k0 ^ 0x736f6d6570736575,
        k1 ^ 0x646f72616e646f6d,
        k0 ^ 0x6c7967656e657261,
        k1 ^ 0x7465646279746573,
    ]
}

#[inline(always)]
fn sip13_block(v: &mut [u64; 4], block: u64) {
    v[3] ^= block;
    sip_round(v);
    v[0] ^= block;
}

/// Absorb the final block, which holds the last `len % 8` message bytes and
/// `len` in its top byte, then finalize.
#[inline(always)]
fn sip13_finish(mut v: [u64; 4], last: u64) -> u64 {
    sip13_block(&mut v, last);
    v[2] ^= 0xff;
    for _ in 0..3 {
        sip_round(&mut v);
    }
    v[0] ^ v[1] ^ v[2] ^ v[3]
}

/// `SipHasher13::hash(&word.to_le_bytes())` without the general tail
/// handling: one message block and the length-8 final block.
fn sip13_word(key: &[u8; 16], word: u64) -> u64 {
    let mut v = sip13_start(key);
    sip13_block(&mut v, word);
    sip13_finish(v, 8 << 56)
}

/// `SipHasher13::hash(bytes)` without the streaming hasher's buffer: whole
/// blocks from the slice, then the tail from fixed-width loads (issue #931).
#[inline(always)]
fn sip13_bytes(key: &[u8; 16], bytes: &[u8]) -> u64 {
    let mut v = sip13_start(key);
    let (blocks, rest) = bytes.as_chunks::<8>();
    for block in blocks {
        sip13_block(&mut v, u64::from_le_bytes(*block));
    }
    let tail = if let Some(last) = bytes.last_chunk::<8>() {
        // The slice's last eight bytes end with the tail; shift out the
        // bytes before it. A whole number of blocks leaves no tail.
        match rest.len() {
            0 => 0,
            count => u64::from_le_bytes(*last) >> (8 * (8 - count)),
        }
    } else {
        // Fewer than eight bytes: two loads that overlap in the middle.
        match rest.len() {
            0 => 0,
            1 => u64::from(rest[0]),
            len @ (2 | 3) => {
                let (first, last) = ([rest[0], rest[1]], [rest[len - 2], rest[len - 1]]);
                u64::from(u16::from_le_bytes(first))
                    | u64::from(u16::from_le_bytes(last)) << (8 * (len - 2))
            }
            len => {
                let first = [rest[0], rest[1], rest[2], rest[3]];
                let last = [rest[len - 4], rest[len - 3], rest[len - 2], rest[len - 1]];
                u64::from(u32::from_le_bytes(first))
                    | u64::from(u32::from_le_bytes(last)) << (8 * (len - 4))
            }
        }
    };
    sip13_finish(v, tail | (bytes.len() as u64) << 56)
}

#[inline(always)]
fn sip_round(v: &mut [u64; 4]) {
    v[0] = v[0].wrapping_add(v[1]);
    v[1] = v[1].rotate_left(13) ^ v[0];
    v[0] = v[0].rotate_left(32);
    v[2] = v[2].wrapping_add(v[3]);
    v[3] = v[3].rotate_left(16) ^ v[2];
    v[0] = v[0].wrapping_add(v[3]);
    v[3] = v[3].rotate_left(21) ^ v[0];
    v[2] = v[2].wrapping_add(v[1]);
    v[1] = v[1].rotate_left(17) ^ v[2];
    v[2] = v[2].rotate_left(32);
}

/// Scalar writes through references update the declared width only; padding
/// in a record or projected sum slot is not part of its value.
fn scalar(value: u128, kind: u8) -> u64 {
    match kind {
        b'1' | b'5' | b'b' => value as u8 as u64,
        b'2' | b'6' => value as u16 as u64,
        b'3' | b'7' => value as u32 as u64,
        _ => value as u64,
    }
}

unsafe fn write_key(state: &mut SipHasher13, value: u128, ty: &Type) {
    state.write_u8(ty.kind);
    if ty.kind == b'C' {
        if let Some(record) = ty.record(value) {
            state.write_usize(record.len());
            for (field, ty) in record.slots() {
                // SAFETY: the record view belongs to this live typed key.
                unsafe { write_key(state, field.read(), ty) };
            }
        } else {
            state.write_usize(0);
        }
    } else if ty.kind == b'B' {
        state.write_u64(ty.tag(value) as u64);
        if let Some(record) = ty.record(value) {
            state.write_usize(record.len());
            for (field, ty) in record.slots() {
                // SAFETY: the record view belongs to this live typed key.
                unsafe { write_key(state, field.read(), ty) };
            }
        } else if let Some(field) = ty.payload(value) {
            state.write_usize(1);
            // SAFETY: the active variant's payload has this descriptor.
            unsafe { write_key(state, ty.unpack(value), field) };
        } else {
            state.write_usize(0);
        }
    } else if matches!(ty.kind, b's' | b'y') {
        // SAFETY: str/bytes keys hold a live Text pointer.
        let text = value as *const Text;
        let bytes = unsafe { strings::bytes(&text) };
        // Length framing distinguishes e.g. ("ab", "c") from ("a", "bc").
        state.write_usize(bytes.len());
        state.write(bytes);
    } else {
        state.write_u64(scalar(value, ty.kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siphash13_matches_the_reference_vectors() {
        // Vectors 0, 8 and 15 of the SipHash-1-3 reference suite: key 00..0f,
        // message 00..(n-1).
        let key = std::array::from_fn(|i| i as u8);
        let message: [u8; 15] = std::array::from_fn(|i| i as u8);
        let hasher = SipHasher13::new_with_key(&key);
        assert_eq!(hasher.hash(&[]), 0xabac0158050fc4dc);
        assert_eq!(hasher.hash(&message[..8]), 0x369095118d299a8e);
        assert_eq!(hasher.hash(&message), 0xd320d86d2a519956);
    }

    #[test]
    fn word_hash_matches_the_general_implementation() {
        let mut x = 0x9e3779b97f4a7c15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..1000 {
            let mut key = [0; 16];
            key[..8].copy_from_slice(&next().to_le_bytes());
            key[8..].copy_from_slice(&next().to_le_bytes());
            for word in [0, 1, u64::MAX, next()] {
                assert_eq!(
                    sip13_word(&key, word),
                    SipHasher13::new_with_key(&key).hash(&word.to_le_bytes())
                );
            }
        }
    }

    // Issue #931: the direct text paths compute the same SipHash-1-3 as the
    // streaming hasher at every tail length, for heap and inline texts.
    #[test]
    fn text_hashes_match_the_general_implementation() {
        static TEXT: Type = Type {
            kind: b's',
            affine: false,
            reflexive: true,
            inline_range: false,
            inline_bytes: 0,
            key: None,
            value: None,
            name: "",
            variants: &[],
            drop: None,
            wrapper: None,
            plan: None,
            redacted: false,
            transparent: false,
            debug: None,
            display: None,
            quantity: None,
            finite: None,
            destroy: None,
        };
        let message: [u8; 40] = std::array::from_fn(|i| (i as u8).wrapping_mul(37) ^ 0x5a);
        for key in [[0; 16], [0xa5; 16], std::array::from_fn(|i| i as u8)] {
            let general = SipHasher13::new_with_key(&key);
            for len in 0..=message.len() {
                let bytes = &message[..len];
                assert_eq!(sip13_bytes(&key, bytes), general.hash(bytes), "{len}");
            }
            for len in 0..=strings::INLINE_MAX {
                let ascii: Vec<u8> = message[..len].iter().map(|byte| byte & 0x7f).collect();
                let text = strings::new(&ascii);
                assert!(strings::is_inline(text));
                // SAFETY: an inline text owns nothing.
                let hash = unsafe { hash_with_key(text as u128, &TEXT, &key) };
                assert_eq!(hash, general.hash(&ascii), "{len}");
            }
        }
    }

    #[test]
    fn entropy_retries_interruptions_and_short_reads() {
        let mut calls = 0;
        let key = read_key(|target| {
            calls += 1;
            if calls == 1 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let count = target.len().min(3);
            target[..count].fill(0xa5);
            Ok(count as u128)
        })
        .unwrap();
        assert_eq!(key, [0xa5; 16]);
        assert_eq!(calls, 7);
    }

    #[test]
    fn entropy_failures_never_produce_a_key() {
        for count in [0, 17, u128::MAX] {
            assert!(read_key(|_| Ok(count)).is_err());
        }
        assert_eq!(
            read_key(|_| Err(io::ErrorKind::PermissionDenied.into()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn concurrent_initialization_publishes_one_complete_key() {
        let key = OnceLock::new();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    assert_eq!(
                        key.get_or_init(|| {
                            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            read_key(|target| {
                                target.fill(0x5a);
                                Ok(target.len() as u128)
                            })
                            .unwrap()
                        }),
                        &[0x5a; 16]
                    );
                });
            }
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
}

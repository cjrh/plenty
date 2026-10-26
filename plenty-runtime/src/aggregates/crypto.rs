//! Native back end of `std.crypto`: SHA-256, SHA-512 and HMAC from the
//! RustCrypto `sha2` and `hmac` crates, and constant-time comparison from
//! `subtle`. Nothing here implements a primitive.
//!
//! A hasher lives in a runtime object whose prefix is byte storage, so
//! `std.crypto` holds it as an ordinary `Bytes` that every byte operation sees
//! as empty. Its state never enters a byte buffer: user code can build any
//! byte buffer, so every operation first checks the object's destructor and
//! stops the process on anything else. Finishing and destruction both
//! overwrite the hasher, which holds the HMAC key's padded forms.
use super::byte_storage::{Storage, range, result, size};
use crate::memory::{self, AllocError, Header};
use crate::strings;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use std::ptr;
use subtle::ConstantTimeEq;

/// Algorithm tags shared with `std.crypto`.
const SHA256: u128 = 1;
const SHA512: u128 = 2;
const HMAC_SHA256: u128 = 3;
const HMAC_SHA512: u128 = 4;

// None of these variants owns heap memory or has drop glue, so scrubbing may
// overwrite a live value without dropping it.
enum Hasher {
    Finished,
    Sha256(Sha256),
    Sha512(Sha512),
    HmacSha256(Hmac<Sha256>),
    HmacSha512(Hmac<Sha512>),
}

#[repr(C)]
struct State {
    storage: Storage,
    hasher: Hasher,
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    // SAFETY: the last owner has gone; this runs once, and `pointer` was
    // allocated by `create` with `State`'s layout.
    unsafe {
        let state = pointer.cast::<State>();
        ptr::drop_in_place(ptr::addr_of_mut!((*state).storage.bytes));
        scrub(ptr::addr_of_mut!((*state).hasher));
        memory::free::<State, u8>(state, 0);
    }
}

/// Volatile writes, so the compiler cannot drop the stores as dead. Copies the
/// hashing crates made on the stack are not reached.
unsafe fn scrub(hasher: *mut Hasher) {
    // SAFETY: `hasher` is valid for writes of its own size.
    unsafe {
        let bytes = hasher.cast::<u8>();
        for offset in 0..size_of::<Hasher>() {
            ptr::write_volatile(bytes.add(offset), 0);
        }
        ptr::write(hasher, Hasher::Finished);
    }
}

fn create(hasher: Hasher) -> Result<u128, AllocError> {
    let owner = memory::try_allocate::<State, u8>(0)?;
    // SAFETY: fresh, exclusively owned memory with `State`'s layout.
    unsafe {
        owner.write(State {
            storage: Storage {
                header: Header::new(destroy),
                bytes: Vec::new(),
            },
            hasher,
        });
    }
    Ok(owner as u128)
}

fn start(kind: u128) -> Hasher {
    match kind {
        SHA256 => Hasher::Sha256(Sha256::new()),
        SHA512 => Hasher::Sha512(Sha512::new()),
        _ => crate::fail("unknown digest algorithm"),
    }
}

fn keyed(kind: u128, key: &[u8]) -> Hasher {
    // HMAC accepts every key length: longer keys are hashed first.
    fn accepted<T, E>(mac: Result<T, E>) -> T {
        mac.unwrap_or_else(|_| crate::fail("HMAC rejected its key"))
    }
    match kind {
        HMAC_SHA256 => Hasher::HmacSha256(accepted(Hmac::new_from_slice(key))),
        HMAC_SHA512 => Hasher::HmacSha512(accepted(Hmac::new_from_slice(key))),
        _ => crate::fail("unknown HMAC algorithm"),
    }
}

/// The hasher of a live crypto state, or a stopped process for any other
/// object. Returns a field pointer, never a reference to the whole object, so
/// a byte view of the same object may coexist.
unsafe fn hasher(value: u128) -> *mut Hasher {
    let state = value as *mut State;
    // SAFETY: every byte-storage-typed operand starts with a live `Header`.
    let owner = unsafe { (*state.cast::<Header>()).destroy };
    let ours: unsafe extern "C" fn(*mut Header) = destroy;
    if owner.map(|f| f as usize) != Some(ours as usize) {
        crate::fail("operand is not a std.crypto state");
    }
    // SAFETY: the destructor identifies a `State` allocated by `create`.
    unsafe { ptr::addr_of_mut!((*state).hasher) }
}

/// Absorbs input; false once finished.
unsafe fn update(state: u128, data: &[u8]) -> bool {
    // SAFETY: `hasher` validated the object; `data` lies in a separate buffer.
    match unsafe { &mut *hasher(state) } {
        Hasher::Finished => return false,
        Hasher::Sha256(h) => Digest::update(h, data),
        Hasher::Sha512(h) => Digest::update(h, data),
        Hasher::HmacSha256(h) => Mac::update(h, data),
        Hasher::HmacSha512(h) => Mac::update(h, data),
    }
    true
}

/// Moves the hasher out and scrubs its slot, so finishing happens once.
unsafe fn take(state: u128) -> Hasher {
    // SAFETY: `hasher` validated the object; the copy is the only live value
    // once the slot is scrubbed.
    unsafe {
        let slot = hasher(state);
        let taken = ptr::read(slot);
        scrub(slot);
        taken
    }
}

/// Writes the output at `target[offset..]`; false once finished.
unsafe fn finish(state: u128, target: &mut [u8], offset: usize) -> bool {
    let mut write = |output: &[u8]| match offset
        .checked_add(output.len())
        .and_then(|end| target.get_mut(offset..end))
    {
        Some(place) => place.copy_from_slice(output),
        None => crate::fail("byte range out of bounds"),
    };
    // SAFETY: forwarded from the caller.
    match unsafe { take(state) } {
        Hasher::Finished => return false,
        Hasher::Sha256(h) => write(&h.finalize()),
        Hasher::Sha512(h) => write(&h.finalize()),
        Hasher::HmacSha256(h) => write(&h.finalize().into_bytes()),
        Hasher::HmacSha512(h) => write(&h.finalize().into_bytes()),
    }
    true
}

/// 1 when the output equals `expected`, 0 when not, -1 once finished. The
/// comparison's time depends only on the lengths.
unsafe fn verify(state: u128, expected: &[u8]) -> i64 {
    // SAFETY: forwarded from the caller.
    let equal = match unsafe { take(state) } {
        Hasher::Finished => return -1,
        Hasher::Sha256(h) => bool::from(h.finalize().as_slice().ct_eq(expected)),
        Hasher::Sha512(h) => bool::from(h.finalize().as_slice().ct_eq(expected)),
        Hasher::HmacSha256(h) => h.verify_slice(expected).is_ok(),
        Hasher::HmacSha512(h) => h.verify_slice(expected).is_ok(),
    };
    i64::from(equal)
}

/// Operands are live values of the statically checked signature; `std.bytes`
/// has validated each range, and the runtime repeats the check.
// Inlined so each operation's entry symbol keeps only its own arm.
#[inline(always)]
pub(super) unsafe fn call(op: i64, operands: [u128; 4]) -> u128 {
    let [a, b, c, d] = operands;
    // SAFETY: see above. Byte views borrow a buffer's heap allocation, never
    // the object holding a hasher.
    unsafe {
        let view = |value: u128| (*(value as *const Storage)).bytes.as_slice();
        let slice = |value: u128, start: u128, stop: u128| {
            let bytes = view(value);
            &bytes[range(start, stop, bytes.len())]
        };
        match op {
            300 => result(create(start(a))),
            301 => result(create(keyed(a, slice(b, c, d)))),
            302 => update(a, slice(b, c, d)) as u128,
            303 => {
                let text = b as *const strings::Text;
                update(a, strings::bytes(&text)) as u128
            }
            304 => {
                let target = &mut (*(b as *mut Storage)).bytes;
                finish(a, target, size(c)) as u128
            }
            305 => verify(a, slice(b, c, d)) as u128,
            306 => bool::from(view(a).ct_eq(view(b))) as u128,
            _ => crate::fail("unknown crypto operation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn finishing_scrubs_and_marks_the_state() {
        let state = create(keyed(HMAC_SHA256, b"key")).unwrap();
        unsafe {
            assert!(update(
                state,
                b"The quick brown fox jumps over the lazy dog"
            ));
            let mut out = vec![0; 32];
            assert!(finish(state, &mut out, 0));
            assert_eq!(
                hex(&out),
                "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
            );
            assert!(matches!(*hasher(state), Hasher::Finished));
            assert!(!update(state, b"more"));
            assert!(!finish(state, &mut out, 0));
            assert_eq!(verify(state, &out), -1);
            destroy(state as *mut Header);
        }
    }

    #[test]
    fn verification_rejects_other_lengths_and_contents() {
        let digest = |data: &[u8]| {
            let state = create(start(SHA256)).unwrap();
            unsafe { update(state, data) };
            state
        };
        let expected = Sha256::digest(b"abc");
        unsafe {
            for (data, tag, answer) in [
                (&b"abc"[..], &expected[..], 1),
                (b"abd", &expected[..], 0),
                (b"abc", &expected[..31], 0),
            ] {
                let state = digest(data);
                assert_eq!(verify(state, tag), answer);
                destroy(state as *mut Header);
            }
        }
    }
}

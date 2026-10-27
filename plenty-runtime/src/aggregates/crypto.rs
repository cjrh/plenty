//! Native back end of `std.crypto`: SHA-256, SHA-512, legacy SHA-1 and HMAC
//! from the RustCrypto `sha2`, `sha1` and `hmac` crates, BLAKE3 from the
//! BLAKE3 team's `blake3` crate, and constant-time comparison from `subtle`.
//! Nothing here implements a primitive.
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
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::ptr;
use subtle::ConstantTimeEq;

/// Algorithm tags shared with `std.crypto`.
const SHA256: u128 = 1;
const SHA512: u128 = 2;
const HMAC_SHA256: u128 = 3;
const HMAC_SHA512: u128 = 4;
const SHA1: u128 = 5;
const HMAC_SHA1: u128 = 6;
const BLAKE3: u128 = 7;
const BLAKE3_KEYED: u128 = 8;

// BLAKE3's fixed-capacity tree stack is substantially larger than SHA's
// state. A separate allocation layout keeps existing digest objects small.
#[repr(C)]
struct Blake3State {
    storage: Storage,
    hasher: Option<blake3::Hasher>,
}

unsafe extern "C" fn destroy_blake3(pointer: *mut Header) {
    // SAFETY: this destructor belongs only to create_blake3 allocations and
    // runs after their last owner is released.
    unsafe {
        let state = pointer.cast::<Blake3State>();
        ptr::drop_in_place(ptr::addr_of_mut!((*state).storage.bytes));
        scrub_blake3(ptr::addr_of_mut!((*state).hasher));
        memory::free::<Blake3State, u8>(state, 0);
    }
}

unsafe fn scrub_blake3(slot: *mut Option<blake3::Hasher>) {
    // SAFETY: the slot is exclusively writable. blake3 1.8.7's Hasher owns
    // only inline values, including an ArrayVec of bytes, and no resources
    // requiring destruction. Volatile writes erase the entire stored state;
    // they cannot erase copies the upstream implementation made on the stack.
    unsafe {
        for offset in 0..size_of::<Option<blake3::Hasher>>() {
            ptr::write_volatile(slot.cast::<u8>().add(offset), 0);
        }
        ptr::write(slot, None);
    }
}

fn create_blake3(hasher: blake3::Hasher) -> Result<u128, AllocError> {
    let owner = memory::try_allocate::<Blake3State, u8>(0)?;
    // SAFETY: fresh allocation with the matching layout and exclusive owner.
    unsafe {
        owner.write(Blake3State {
            storage: Storage {
                header: Header::new(destroy_blake3),
                bytes: Vec::new(),
            },
            hasher: Some(hasher),
        });
    }
    Ok(owner as u128)
}

unsafe fn blake3_slot(value: u128) -> Option<*mut Option<blake3::Hasher>> {
    let state = value as *mut Blake3State;
    // SAFETY: all byte-storage operands start with a live Header. Only this
    // destructor identifies the larger Blake3State layout.
    unsafe {
        let owner = (*state.cast::<Header>()).destroy;
        let ours: unsafe extern "C" fn(*mut Header) = destroy_blake3;
        (owner.map(|f| f as usize) == Some(ours as usize))
            .then(|| ptr::addr_of_mut!((*state).hasher))
    }
}

unsafe fn finish_blake3(slot: *mut Option<blake3::Hasher>) -> Option<blake3::Hash> {
    // SAFETY: validated, exclusively owned state. Finalization allocates
    // nothing, and the stored key/tree state is erased even on a mismatch.
    unsafe {
        let output = (*slot).as_ref().map(blake3::Hasher::finalize);
        scrub_blake3(slot);
        output
    }
}

// None of these variants owns heap memory or has drop glue, so scrubbing may
// overwrite a live value without dropping it.
enum Hasher {
    Finished,
    Sha256(Sha256),
    Sha512(Sha512),
    HmacSha256(Hmac<Sha256>),
    HmacSha512(Hmac<Sha512>),
    Sha1(Sha1),
    HmacSha1(Hmac<Sha1>),
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
        SHA1 => Hasher::Sha1(Sha1::new()),
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
        HMAC_SHA1 => Hasher::HmacSha1(accepted(Hmac::new_from_slice(key))),
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
    // SAFETY: the destructor check validates the optional BLAKE3 layout.
    if let Some(slot) = unsafe { blake3_slot(state) } {
        // SAFETY: the caller exclusively borrows the state for this operation.
        return match unsafe { &mut *slot } {
            Some(hasher) => {
                hasher.update(data);
                true
            }
            None => false,
        };
    }
    // SAFETY: `hasher` validated the object; `data` lies in a separate buffer.
    match unsafe { &mut *hasher(state) } {
        Hasher::Finished => return false,
        Hasher::Sha256(h) => Digest::update(h, data),
        Hasher::Sha512(h) => Digest::update(h, data),
        Hasher::HmacSha256(h) => Mac::update(h, data),
        Hasher::HmacSha512(h) => Mac::update(h, data),
        Hasher::Sha1(h) => Digest::update(h, data),
        Hasher::HmacSha1(h) => Mac::update(h, data),
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
        None => crate::fail_kind(
            crate::actor_heap::TrapKind::Index,
            "byte range out of bounds",
        ),
    };
    // SAFETY: forwarded from the caller; the slot is validated before access.
    if let Some(slot) = unsafe { blake3_slot(state) } {
        return match unsafe { finish_blake3(slot) } {
            Some(output) => {
                write(output.as_bytes());
                true
            }
            None => false,
        };
    }
    // SAFETY: forwarded from the caller.
    match unsafe { take(state) } {
        Hasher::Finished => return false,
        Hasher::Sha256(h) => write(&h.finalize()),
        Hasher::Sha512(h) => write(&h.finalize()),
        Hasher::HmacSha256(h) => write(&h.finalize().into_bytes()),
        Hasher::HmacSha512(h) => write(&h.finalize().into_bytes()),
        Hasher::Sha1(h) => write(&h.finalize()),
        Hasher::HmacSha1(h) => write(&h.finalize().into_bytes()),
    }
    true
}

/// 1 when the output equals `expected`, 0 when not, -1 once finished. The
/// comparison's time depends only on the lengths.
unsafe fn verify(state: u128, expected: &[u8]) -> i64 {
    // SAFETY: forwarded from the caller; the slot is validated before access.
    if let Some(slot) = unsafe { blake3_slot(state) } {
        return match unsafe { finish_blake3(slot) } {
            Some(output) => i64::from(bool::from(output.as_bytes().ct_eq(expected))),
            None => -1,
        };
    }
    // SAFETY: forwarded from the caller.
    let equal = match unsafe { take(state) } {
        Hasher::Finished => return -1,
        Hasher::Sha256(h) => bool::from(h.finalize().as_slice().ct_eq(expected)),
        Hasher::Sha512(h) => bool::from(h.finalize().as_slice().ct_eq(expected)),
        Hasher::HmacSha256(h) => h.verify_slice(expected).is_ok(),
        Hasher::HmacSha512(h) => h.verify_slice(expected).is_ok(),
        Hasher::Sha1(h) => bool::from(h.finalize().as_slice().ct_eq(expected)),
        Hasher::HmacSha1(h) => h.verify_slice(expected).is_ok(),
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
            300 if a == BLAKE3 => result(create_blake3(blake3::Hasher::new())),
            300 => result(create(start(a))),
            301 if a == BLAKE3_KEYED => {
                let key = slice(b, c, d)
                    .try_into()
                    .unwrap_or_else(|_| crate::fail("BLAKE3 key must contain 32 bytes"));
                result(create_blake3(blake3::Hasher::new_keyed(key)))
            }
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
            307 => {
                let text = a as *const strings::Text;
                // Plenty strings always contain valid UTF-8.
                let context = std::str::from_utf8_unchecked(strings::bytes(&text));
                result(create_blake3(blake3::Hasher::new_derive_key(context)))
            }
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
    fn blake3_finishes_once_and_validates_both_state_layouts() {
        let state = create_blake3(blake3::Hasher::new_keyed(
            b"whats the Elvish word for friend",
        ))
        .unwrap();
        let sha = create(start(SHA256)).unwrap();
        // SAFETY: both objects were allocated above; the matching destructor
        // releases each once after all test borrows have ended.
        unsafe {
            assert!(blake3_slot(sha).is_none());
            let slot = blake3_slot(state).unwrap();
            let mut output = [0; 32];
            assert!(finish(state, &mut output, 0));
            assert_eq!(
                hex(&output),
                "92b2b75604ed3c761f9d6f62392c8a9227ad0ea3f09573e783f1498a4ed60d26"
            );
            assert!((*slot).is_none());
            assert!(!update(state, b"more"));
            assert!(!finish(state, &mut output, 0));
            assert_eq!(verify(state, &output), -1);
            destroy_blake3(state as *mut Header);
            destroy(sha as *mut Header);
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

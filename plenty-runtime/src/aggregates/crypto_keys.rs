//! RSA and P-256 keys over `ring`. `std.crypto` parses public-key encodings
//! and validates RSA components in Plenty; this module holds private keys
//! parsed from PKCS#8 and runs ring's signing and verification. ECDSA paths
//! allocate nothing. ring's RSA key parsing, signing and verification allocate
//! through the Rust global allocator, which cannot report failure: see
//! book/src/design/stdlib/crypto-keys.md.
use super::byte_storage::{Storage, result};
use crate::memory::{self, AllocError, Header};
use ring::{agreement, rand, rsa, signature};
use signature::KeyPair as _;
use std::ptr;

// Status ABI, shared with `std.crypto`'s advanced_status: 1 success, 0 bad
// signature, -1 invalid length, -3 entropy failure, -5 invalid key, -6 key
// too small.
const INVALID_LENGTH: i64 = -1;
const RANDOM_FAILURE: i64 = -3;
const INVALID_KEY: i64 = -5;
const WEAK_KEY: i64 = -6;

enum Key {
    Empty,
    Rsa(rsa::KeyPair),
    P256(signature::EcdsaKeyPair),
}

#[repr(C)]
struct Private {
    storage: Storage,
    key: Key,
}

/// Overwrites the key slot. `EcdsaKeyPair` has no drop glue, so its scalar
/// and nonce key are scrubbed in place. ring's RSA limbs live in boxes that
/// ring frees without zeroing; only the boxes' pointers are scrubbed here.
unsafe fn clear_key(slot: *mut Key) {
    const { assert!(!std::mem::needs_drop::<signature::EcdsaKeyPair>()) };
    // SAFETY: the caller owns this initialized slot and writes a valid value
    // before it is read again.
    unsafe {
        if matches!(*slot, Key::Rsa(_)) {
            ptr::drop_in_place(slot);
        }
        for i in 0..size_of::<Key>() {
            ptr::write_volatile(slot.cast::<u8>().add(i), 0);
        }
        slot.write(Key::Empty);
    }
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    // SAFETY: called exactly once for a Private allocated by private().
    unsafe {
        let state = pointer.cast::<Private>();
        clear_key(ptr::addr_of_mut!((*state).key));
        ptr::drop_in_place(ptr::addr_of_mut!((*state).storage.bytes));
        memory::free::<Private, u8>(state, 0);
    }
}

/// An empty key state from the runtime's fallible allocator. Loading a key
/// into it happens separately, so allocation failure and key rejection stay
/// distinct results.
fn private() -> Result<u128, AllocError> {
    let state = memory::try_allocate::<Private, u8>(0)?;
    // SAFETY: a fresh allocation with the correct layout.
    unsafe {
        state.write(Private {
            storage: Storage {
                header: Header::new(destroy),
                bytes: Vec::new(),
            },
            key: Key::Empty,
        });
    }
    Ok(state as u128)
}

unsafe fn private_state(value: u128) -> *mut Private {
    let state = value as *mut Private;
    // SAFETY: all ByteStorage operands have a live Header prefix.
    if unsafe { (*state.cast::<Header>()).destroy }.map(|f| f as usize)
        != Some(destroy as *const () as usize)
    {
        crate::fail("operand is not a private key");
    }
    state
}

/// ring reports why a key was rejected only through `Display`. Comparing the
/// text in a stack buffer keeps the rejection path allocation-free.
fn rejected_as(error: ring::error::KeyRejected, reason: &[u8]) -> bool {
    struct Text {
        bytes: [u8; 32],
        len: usize,
    }
    impl std::fmt::Write for Text {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            let end = self.len + text.len();
            let slot = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
            slot.copy_from_slice(text.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut text = Text {
        bytes: [0; 32],
        len: 0,
    };
    std::fmt::write(&mut text, format_args!("{error}")).is_ok() && text.bytes[..text.len] == *reason
}

fn rsa_padding(scheme: i64) -> &'static dyn signature::RsaEncoding {
    match scheme {
        0 => &signature::RSA_PKCS1_SHA256,
        1 => &signature::RSA_PKCS1_SHA384,
        2 => &signature::RSA_PKCS1_SHA512,
        3 => &signature::RSA_PSS_SHA256,
        4 => &signature::RSA_PSS_SHA384,
        5 => &signature::RSA_PSS_SHA512,
        _ => crate::fail("unknown RSA signature scheme"),
    }
}

fn rsa_parameters(scheme: i64) -> &'static signature::RsaParameters {
    match scheme {
        0 => &signature::RSA_PKCS1_2048_8192_SHA256,
        1 => &signature::RSA_PKCS1_2048_8192_SHA384,
        2 => &signature::RSA_PKCS1_2048_8192_SHA512,
        3 => &signature::RSA_PSS_2048_8192_SHA256,
        4 => &signature::RSA_PSS_2048_8192_SHA384,
        5 => &signature::RSA_PSS_2048_8192_SHA512,
        _ => crate::fail("unknown RSA signature scheme"),
    }
}

/// Whether `point` is an uncompressed SEC1 P-256 point on the curve. ring
/// exposes no point validator, so this runs one ephemeral ECDH agreement,
/// which validates the peer point before using it.
fn p256_point(point: &[u8]) -> i64 {
    if point.len() != 65 || point[0] != 4 {
        return INVALID_KEY;
    }
    let Ok(key) =
        agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rand::SystemRandom::new())
    else {
        return RANDOM_FAILURE;
    };
    let peer = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, point);
    match agreement::agree_ephemeral(key, &peer, |_| ()) {
        Ok(()) => 1,
        Err(_) => INVALID_KEY,
    }
}

/// Operands follow the compiler signature. Reject output aliases before
/// constructing mutable slices, including direct calls through `std.bytes`.
#[inline(always)]
pub(super) unsafe fn call(op: i64, operands: [u128; 4]) -> u128 {
    let [a, b, c, d] = operands;
    let (output, inputs): (Option<u128>, &[u128]) = match op {
        341 | 342 => (Some(a), &[b]),
        344 => (Some(b), &[a]),
        345 => (Some(d), &[a, c]),
        _ => (None, &[]),
    };
    if output.is_some_and(|output| inputs.contains(&output)) {
        crate::fail("crypto output aliases an input");
    }
    // SAFETY: ByteStorage values refer to live Storage prefixes. Output
    // exclusivity is checked by the compiler and aliases rejected above.
    unsafe {
        let view = |value: u128| (*(value as *const Storage)).bytes.as_slice();
        let target = |value: u128| (*(value as *mut Storage)).bytes.as_mut_slice();
        let status = match op {
            340 => return result(private()),
            341 | 342 => {
                let state = private_state(a);
                if !matches!((*state).key, Key::Empty) {
                    crate::fail("private key state is already loaded");
                }
                let der = view(b);
                let loaded = if op == 341 {
                    rsa::KeyPair::from_pkcs8(der).map(Key::Rsa)
                } else {
                    signature::EcdsaKeyPair::from_pkcs8(
                        &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                        der,
                        &rand::SystemRandom::new(),
                    )
                    .map(Key::P256)
                };
                match loaded {
                    Ok(key) => {
                        (*state).key = key;
                        1
                    }
                    Err(error) if rejected_as(error, b"TooSmall") => WEAK_KEY,
                    // ring reports an entropy failure while deriving the
                    // ECDSA nonce key as a rejection with this text.
                    Err(error) if rejected_as(error, b"RNG failed") => RANDOM_FAILURE,
                    Err(_) => INVALID_KEY,
                }
            }
            343 => {
                let state = &*private_state(a);
                match (&state.key, b as i64) {
                    (Key::Rsa(key), 0) => key.public().as_ref().len() as i64,
                    (Key::Rsa(key), _) => key.public().modulus_len() as i64,
                    (Key::P256(key), 0) => key.public_key().as_ref().len() as i64,
                    (Key::P256(_), _) => 64,
                    (Key::Empty, _) => crate::fail("private key state is empty"),
                }
            }
            344 => {
                let state = &*private_state(a);
                let public = match &state.key {
                    Key::Rsa(key) => key.public().as_ref(),
                    Key::P256(key) => key.public_key().as_ref(),
                    Key::Empty => crate::fail("private key state is empty"),
                };
                let out = target(b);
                if out.len() != public.len() {
                    INVALID_LENGTH
                } else {
                    out.copy_from_slice(public);
                    1
                }
            }
            345 => {
                let state = &*private_state(a);
                let message = view(c);
                let out = target(d);
                let rng = rand::SystemRandom::new();
                match &state.key {
                    Key::Rsa(key) => {
                        let padding = rsa_padding(b as i64);
                        if out.len() != key.public().modulus_len() {
                            INVALID_LENGTH
                        } else if key.sign(padding, &rng, message, out).is_ok() {
                            1
                        } else {
                            // The length is checked, so only PSS salt
                            // generation can fail.
                            out.fill(0);
                            RANDOM_FAILURE
                        }
                    }
                    Key::P256(key) => {
                        if out.len() != 64 {
                            INVALID_LENGTH
                        } else {
                            match key.sign(&rng, message) {
                                Ok(value) => {
                                    out.copy_from_slice(value.as_ref());
                                    1
                                }
                                Err(_) => RANDOM_FAILURE,
                            }
                        }
                    }
                    Key::Empty => crate::fail("private key state is empty"),
                }
            }
            346 => {
                let parameters = rsa_parameters(b as i64);
                i64::from(
                    signature::UnparsedPublicKey::new(parameters, view(a))
                        .verify(view(c), view(d))
                        .is_ok(),
                )
            }
            347 => {
                if view(c).len() != 64 {
                    INVALID_LENGTH
                } else {
                    i64::from(
                        signature::UnparsedPublicKey::new(
                            &signature::ECDSA_P256_SHA256_FIXED,
                            view(a),
                        )
                        .verify(view(b), view(c))
                        .is_ok(),
                    )
                }
            }
            348 => p256_point(view(a)),
            _ => crate::fail("unknown key operation"),
        };
        status as u128
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `call` maps ring's rejection reasons by their Display text; this fails
    // if a ring update renames them.
    #[test]
    fn rejection_reasons_are_recognised() {
        let Err(error) = rsa::KeyPair::from_pkcs8(&[]) else {
            panic!("empty PKCS #8 accepted");
        };
        assert!(rejected_as(error, b"InvalidEncoding"));
        let Err(error) = rsa::KeyPair::from_pkcs8(&[]) else {
            panic!("empty PKCS #8 accepted");
        };
        assert!(!rejected_as(error, b"TooSmall"));
        assert_eq!(p256_point(&[4; 65]), INVALID_KEY);
        assert_eq!(p256_point(&[2; 33]), INVALID_KEY);
    }
}

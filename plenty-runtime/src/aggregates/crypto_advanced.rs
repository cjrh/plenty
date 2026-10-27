//! Curated `ring` primitives. Key objects, tags, signatures and HKDF state
//! use fixed-size stack storage; only X25519's owned state allocates, through
//! the runtime's fallible allocator. No PKCS#8 or allocating seal API is used.
use super::byte_storage::{Storage, result};
use crate::memory::{self, AllocError, Header};
use ring::{aead, agreement, hkdf, rand, signature};
use signature::KeyPair;
use std::ptr;

// Status ABI: 1 success, 0 authentication mismatch, -1 invalid length,
// -2 consumed key, -3 entropy/key-generation failure, -4 invalid peer.
#[repr(C)]
struct Exchange {
    storage: Storage,
    key: Option<agreement::EphemeralPrivateKey>,
    failed: bool,
}

unsafe fn clear_key(slot: *mut Option<agreement::EphemeralPrivateKey>) {
    // SAFETY: caller owns this initialized slot. The key has no drop glue;
    // overwrite it and restore a valid None before the slot is used again.
    const { assert!(!std::mem::needs_drop::<agreement::EphemeralPrivateKey>()) };
    unsafe {
        for i in 0..size_of::<Option<agreement::EphemeralPrivateKey>>() {
            ptr::write_volatile(slot.cast::<u8>().add(i), 0);
        }
        slot.write(None);
    }
}

unsafe extern "C" fn destroy(pointer: *mut Header) {
    // SAFETY: called exactly once for an Exchange allocated by exchange().
    unsafe {
        let state = pointer.cast::<Exchange>();
        clear_key(ptr::addr_of_mut!((*state).key));
        ptr::drop_in_place(ptr::addr_of_mut!((*state).storage.bytes));
        memory::free::<Exchange, u8>(state, 0);
    }
}

fn exchange() -> Result<u128, AllocError> {
    let state = memory::try_allocate::<Exchange, u8>(0)?;
    let key =
        agreement::EphemeralPrivateKey::generate(&agreement::X25519, &rand::SystemRandom::new())
            .ok();
    let failed = key.is_none();
    // SAFETY: a fresh allocation with the correct layout.
    unsafe {
        state.write(Exchange {
            storage: Storage {
                header: Header::new(destroy),
                bytes: Vec::new(),
            },
            key,
            failed,
        });
    }
    Ok(state as u128)
}

unsafe fn exchange_state(value: u128) -> *mut Exchange {
    let state = value as *mut Exchange;
    // SAFETY: all ByteStorage operands have a live Header prefix.
    if unsafe { (*state.cast::<Header>()).destroy }.map(|f| f as usize)
        != Some(destroy as *const () as usize)
    {
        crate::fail("operand is not an X25519 key");
    }
    state
}

fn encrypt(
    algorithm: &'static aead::Algorithm,
    opening: bool,
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    buffer: &mut [u8],
) -> i64 {
    if key.len() != 32 || nonce.len() != 12 || buffer.len() < 16 {
        return -1;
    }
    let Ok(key) = aead::UnboundKey::new(algorithm, key) else {
        return -1;
    };
    let key = aead::LessSafeKey::new(key);
    let nonce = aead::Nonce::assume_unique_for_key(nonce.try_into().unwrap());
    let aad = aead::Aad::from(aad);
    if opening {
        if key.open_in_place(nonce, aad, buffer).is_ok() {
            1
        } else {
            buffer.fill(0);
            0
        }
    } else {
        let (message, tag) = buffer.split_at_mut(buffer.len() - 16);
        match key.seal_in_place_separate_tag(nonce, aad, message) {
            Ok(value) => {
                tag.copy_from_slice(value.as_ref());
                1
            }
            Err(_) => {
                buffer.fill(0);
                0
            }
        }
    }
}

struct OutputLength(usize);
impl hkdf::KeyType for OutputLength {
    fn len(&self) -> usize {
        self.0
    }
}

/// Operands follow the compiler signature. Reject aliases before constructing
/// mutable slices, including direct calls through the public Bytes back end.
#[inline(always)]
pub(super) unsafe fn call(op: i64, operands: [u128; 4]) -> u128 {
    let [a, b, c, d] = operands;
    let (output, inputs): (Option<u128>, &[u128]) = match op {
        308..=311 | 315 => (Some(d), &[a, b, c]),
        312 | 317 => (Some(b), &[a]),
        313 | 318 => (Some(c), &[a, b]),
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
            308..=311 => encrypt(
                if op < 310 {
                    &aead::AES_256_GCM
                } else {
                    &aead::CHACHA20_POLY1305
                },
                op % 2 == 1,
                view(a),
                view(b),
                view(c),
                target(d),
            ),
            312 | 313 => {
                let seed = view(a);
                let out = target(if op == 312 { b } else { c });
                if seed.len() != 32 || out.len() != if op == 312 { 32 } else { 64 } {
                    -1
                } else {
                    match signature::Ed25519KeyPair::from_seed_unchecked(seed) {
                        Ok(key) => {
                            if op == 312 {
                                out.copy_from_slice(key.public_key().as_ref());
                            } else {
                                out.copy_from_slice(key.sign(view(b)).as_ref());
                            }
                            1
                        }
                        Err(_) => -1,
                    }
                }
            }
            314 => {
                if view(a).len() != 32 || view(c).len() != 64 {
                    -1
                } else {
                    i64::from(
                        signature::UnparsedPublicKey::new(&signature::ED25519, view(a))
                            .verify(view(b), view(c))
                            .is_ok(),
                    )
                }
            }
            315 => {
                let out = target(d);
                if out.len() > 255 * 32 {
                    -1
                } else {
                    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, view(a));
                    let prk = salt.extract(view(b));
                    let info = [view(c)];
                    match prk
                        .expand(&info, OutputLength(out.len()))
                        .and_then(|key| key.fill(out))
                    {
                        Ok(()) => 1,
                        Err(_) => -1,
                    }
                }
            }
            316 => return result(exchange()),
            317 => {
                let state = &*exchange_state(a);
                let out = target(b);
                if out.len() != 32 {
                    -1
                } else if state.failed {
                    -3
                } else if let Some(key) = &state.key {
                    match key.compute_public_key() {
                        Ok(public) => {
                            out.copy_from_slice(public.as_ref());
                            1
                        }
                        Err(_) => -3,
                    }
                } else {
                    -2
                }
            }
            318 => {
                let state = exchange_state(a);
                let peer = view(b);
                let out = target(c);
                if peer.len() != 32 || out.len() != 32 {
                    -1
                } else if (*state).failed {
                    -3
                } else {
                    let slot = ptr::addr_of_mut!((*state).key);
                    let key = ptr::read(slot);
                    clear_key(slot);
                    match key {
                        None => -2,
                        Some(key) => {
                            let peer = agreement::UnparsedPublicKey::new(&agreement::X25519, peer);
                            match agreement::agree_ephemeral(key, &peer, |shared| {
                                out.copy_from_slice(shared)
                            }) {
                                Ok(()) => 1,
                                Err(_) => {
                                    out.fill(0);
                                    -4
                                }
                            }
                        }
                    }
                }
            }
            _ => crate::fail("unknown advanced crypto operation"),
        };
        status as u128
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_generation_is_reported_without_touching_output() {
        let state = memory::try_allocate::<Exchange, u8>(0).unwrap();
        // Construct the state produced by a failed OS entropy request. This
        // keeps the production RNG real while exercising the failure path.
        unsafe {
            state.write(Exchange {
                storage: Storage {
                    header: Header::new(destroy),
                    bytes: Vec::new(),
                },
                key: None,
                failed: true,
            });
            let mut output = Storage {
                header: Header::new(destroy),
                bytes: vec![99; 32],
            };
            let pointer = ptr::addr_of_mut!(output) as u128;
            assert_eq!(call(317, [state as u128, pointer, 0, 0]) as i64, -3);
            assert_eq!(output.bytes, [99; 32]);
            destroy(state.cast());
        }
    }

    #[test]
    fn generated_private_slot_is_cleared_on_consumption() {
        let state = exchange().unwrap();
        unsafe {
            let state = exchange_state(state);
            assert!((*state).key.is_some());
            let mut peer = Storage {
                header: Header::new(destroy),
                bytes: vec![0; 32],
            };
            let mut output = Storage {
                header: Header::new(destroy),
                bytes: vec![99; 32],
            };
            let peer = ptr::addr_of_mut!(peer) as u128;
            let output_pointer = ptr::addr_of_mut!(output) as u128;
            assert_eq!(
                call(318, [state as u128, peer, output_pointer, 0]) as i64,
                -4
            );
            assert!((*state).key.is_none());
            assert_eq!(output.bytes, [0; 32]);
            assert_eq!(
                call(318, [state as u128, peer, output_pointer, 0]) as i64,
                -2
            );
            destroy(state.cast());
        }
    }
}

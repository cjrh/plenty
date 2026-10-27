//! Key the complete structural input, rather than an unkeyed prehash whose
//! collisions would survive any finalizer or secret suffix.
use super::{Text, Type, strings};
use siphasher::sip::SipHasher24;
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

pub(super) unsafe fn hash(value: u128, ty: &Type) -> u64 {
    // Creating or reserving an empty collection does not require entropy.
    // OnceLock publishes one complete key to all workers; no allocation or
    // user code is involved. Never substitute predictable bytes on OS failure.
    let key = KEY.get_or_init(|| {
        read_key(crate::entropy::read)
            .unwrap_or_else(|_| crate::fail("collection hashing requires OS entropy"))
    });
    // SAFETY: forwarded from the typed collection operation.
    unsafe { hash_with_key(value, ty, key) }
}

pub(super) unsafe fn hash_with_key(value: u128, ty: &Type, key: &[u8; 16]) -> u64 {
    let mut state = SipHasher24::new_with_key(key);
    // SAFETY: forwarded from the typed collection operation.
    unsafe { write_key(&mut state, value, ty) };
    state.finish()
}

unsafe fn write_key(state: &mut SipHasher24, value: u128, ty: &Type) {
    state.write_u8(ty.kind);
    if ty.kind == b'B' {
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
        // Scalar writes through references update the declared width only;
        // padding in a record or projected sum slot is not part of its value.
        let value = match ty.kind {
            b'1' | b'5' | b'b' => value as u8 as u64,
            b'2' | b'6' => value as u16 as u64,
            b'3' | b'7' => value as u32 as u64,
            _ => value as u64,
        };
        state.write_u64(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siphash24_matches_the_empty_message_reference_vector() {
        let key = std::array::from_fn(|i| i as u8);
        assert_eq!(SipHasher24::new_with_key(&key).finish(), 0x726fdb47dd0e0e31);
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

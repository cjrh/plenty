# Authenticated encryption, signatures, and key agreement

[`std.crypto`](crypto.md) provides AES-256-GCM, ChaCha20-Poly1305,
Ed25519, ephemeral X25519, and HKDF-SHA256 over [`Bytes`](bytes.md).
These are `ring` primitives; Plenty implements argument validation, buffer
ownership, and error handling. Cryptography requires specialist implementation
and review, so this module wraps the existing vendored library instead of
reimplementing primitives in Plenty.

All functions return `Result[_, AdvancedError]`. This error is separate from
`CryptoError`, which describes the digest/HMAC API. See the
[signature index](signatures.md#stdcrypto) for complete declarations and the
[encryption tutorial](../../tutorial/07-modules/11a-standard-encryption.md)
for runnable examples.

## Authenticated encryption

`AeadAlgorithm` has `Aes256Gcm` and `ChaCha20Poly1305`. Both use a 32-byte
secret key, a 12-byte nonce, and a 16-byte authentication tag.

```text
seal_in_place(algorithm, key: &Bytes, nonce: &Bytes, aad: &Bytes,
              data: &mut Bytes) -> Result[(), AdvancedError]
open_in_place(algorithm, key: &Bytes, nonce: &Bytes, aad: &Bytes,
              data: &mut Bytes) -> Result[(), AdvancedError]
```

`seal_in_place` replaces plaintext with ciphertext followed by its tag. It
reserves/grows the buffer before encrypting; reserve 16 additional bytes in
advance to avoid allocation. `open_in_place` authenticates and decrypts that
format, removing the tag on success. Associated data (`aad`) is authenticated
but never encrypted or appended; the recipient must supply identical bytes.
Empty plaintext and empty associated data are supported. These APIs cap
plaintext at 68,719,476,704 bytes for both algorithms.

**Never reuse a nonce with the same key.** The API does not remember nonces
or manage counters, persistent state, key rotation, replay protection, or
message framing. Use a protocol that specifies these. Keys should come from
[`std.secrets`](secrets.md) or an appropriate key derivation scheme.

Invalid lengths leave `data` unchanged. If growth fails, plaintext remains
unchanged. Authentication failure zeros the **entire** ciphertext/tag buffer,
retains its original length, and returns `AuthenticationFailed`. It never
returns unauthenticated plaintext. Successful opening allocates nothing.
The other inputs and output must be separate buffers under the loan rules.

## Ed25519

| Function | Output |
|---|---|
| `ed25519_public_key(seed)` | A new 32-byte public key |
| `ed25519_public_key_into(seed, target)` | Writes an exactly 32-byte buffer |
| `ed25519_sign(seed, message)` | A new 64-byte signature |
| `ed25519_sign_into(seed, message, target)` | Writes an exactly 64-byte buffer |
| `ed25519_verify(public_key, message, signature)` | `True` for a valid signature; `False` for a mismatch; `SmallOrderKey` for a [small-order key](#small-order-public-keys) |

Every input is `&Bytes`; `target` is `&mut Bytes`. A seed must be exactly
32 cryptographically random bytes. Generate one with `std.secrets.token_bytes(32)`;
its public key may be shared, but its seed must remain secret. Signing is
deterministic, and a signing seed can sign multiple messages. Verification
requires exactly 32 public-key bytes and 64 signature bytes; other lengths
return `InvalidLength`, whereas correctly sized malformed keys/signatures
return `False`. This is plain Ed25519, without a prehash or context mode.
Public keys must be authenticated by the application's protocol.

### Small-order public keys

`ed25519_verify` and [SPKI import](crypto-keys.md#import) reject a
small-order public key with `SmallOrderKey`. Such a key is a point whose
order divides 8; no private key stands behind it, so anyone can make
signatures that verify under it. Under the identity point, the signature
`R = B`, `S = 1` verifies for every message. The rule covers all 14
encodings of the eight points, including the non-canonical ones (`y = p` and
`y = p + 1`) and both sign bits. Verification checks lengths first, so a
small-order key with a wrong-length signature is still `InvalidLength`. The
result is an error rather than `False` because no answer about the signature
is meaningful for such a key.

```plenty
import std.bytes
import std.crypto
import std.encoding
import std.report

def main() -> Result[(), IoError | std.bytes.BytesError | std.encoding.EncodingError | std.report.FormatError]:
    identity = std.encoding.hex_from_text("0100000000000000000000000000000000000000000000000000000000000000")?
    forged = std.encoding.hex_from_text("58666666666666666666666666666666666666666666666666666666666666660100000000000000000000000000000000000000000000000000000000000000")?
    message = std.bytes.from_text("any message")?
    match std.crypto.ed25519_verify(&identity, &message, &forged):
        case Ok(valid):
            print(valid)?
        case Err(error):
            print(std.report.to_string(&error, 256, 4)?)?
    Ok(())
```
```output
Ed25519 public key has small order: signatures can verify under it without any private key, so it is rejected
```

[RFC 8032](https://www.rfc-editor.org/rfc/rfc8032) permits these keys and
`ring` accepts them, so a peer that publishes one interoperates with RFC 8032
verifiers but not with Plenty. libsodium rejects the same set.

Mixed-order keys, a prime-order point plus a small-order one, are accepted.
Signing under one still needs the private key of its prime-order part, so
they do not allow forgery. They do let a key holder produce signatures that
verifiers with and without the cofactor in their equation judge differently.
Rejecting them needs a scalar multiplication by the group order, which `ring`
does not expose; libsodium's verification accepts them too. The
[Wycheproof vectors](crypto-keys.md#test-vectors) pin both rules.

## X25519

`x25519() -> Result[X25519Key, AdvancedError]` generates an ephemeral private
key with `ring`'s operating-system random source. The private key has no
export/import operation. An `X25519Key` has these methods:

| Method | Contract |
|---|---|
| `public_key()` | Returns a new 32-byte buffer |
| `public_key_into(target: &mut Bytes)` | Writes an exactly 32-byte buffer |
| `agree(peer_public_key: &Bytes)` | Consumes the key's agreement capability and returns 32 shared-secret bytes |
| `agree_into(peer_public_key: &Bytes, target: &mut Bytes)` | Same, into an exactly 32-byte buffer |

Both agreement methods require a mutable receiver. An agreement attempt
consumes the private key even when the peer is rejected. A peer producing an
all-zero shared secret is `InvalidPeer`; `agree_into` zeros its output on
that failure. Later agreement or public-key requests return `Finished`.
Invalid lengths and output allocation failure leave the key usable and
existing output unchanged. Exchange public keys before calling `agree`.

X25519 alone does **not** authenticate the peer. Authenticate the exchange
through a suitable protocol and pass the shared secret through HKDF with
protocol-specific salt and context before using it as a symmetric key.

## HKDF-SHA256

```text
hkdf_sha256(salt: &Bytes, key_material: &Bytes, info: &Bytes, size: i64)
    -> Result[Bytes, AdvancedError]
hkdf_sha256_into(salt: &Bytes, key_material: &Bytes, info: &Bytes,
                 target: &mut Bytes) -> Result[(), AdvancedError]
```

These perform HKDF extract followed by expand. Salt and context (`info`) can
be empty. Output length is 0 through 8160 bytes inclusive; the `_into` form
uses the existing target length. Invalid lengths leave existing output
unchanged. Use context to separate keys with different purposes. HKDF is
**not password hashing** and does not make a weak password into a strong key.

## Errors, allocation, and key material

| `AdvancedError` | Meaning |
|---|---|
| `InvalidLength` | Incorrect key/nonce/tag/output length or output beyond the supported bound |
| `AuthenticationFailed` | AEAD authentication failed |
| `InvalidPeer` | X25519 rejected the peer key |
| `Finished` | X25519 key already used for an agreement attempt |
| `RandomFailure` | OS entropy or X25519 key/public-key generation failed |
| `InvalidKey`, `WeakKey` | Raised only by [RSA, ECDSA and key import](crypto-keys.md#errors) |
| `SmallOrderKey` | An Ed25519 public key has [small order](#small-order-public-keys) |
| `Allocation(AllocError)` | A fallible buffer or private-state allocation failed |

`AdvancedError` satisfies [`std.report.Formattable`](report.md): each variant
formats as a fixed sentence stating its cause, without key material. An
[entrypoint report](../toolchain/01-building/entry-report.md) uses it when
`main` returns a bare `AdvancedError`; an error sum uses it only when every
member is formattable.

The `_into` functions, verification, and opening allocate nothing. Sealing
allocates only if growth exceeds capacity. Functions returning `Bytes`
allocate their result. X25519 creation fallibly allocates its state and a
short-lived 32-byte public-key buffer to validate generation before returning.
Failure at any of these allocation points is reported as `Allocation`.

The chosen `ring` paths use fixed-size stack key objects, signatures, tags,
and HKDF state. They do not use its allocating PKCS#8 or append-tag APIs.
Tests enforce the allocation-free paths with the runtime's allocation guard.
The X25519 state's private-key slot is overwritten with volatile zero writes
when consumed or dropped. This does not scrub copies in registers or on the
stack, caller-owned seeds/keys/shared-secret buffers, or allocator caches.
`Bytes` does not zero its storage on drop; see [key material](crypto.md#key-material).

There are no password KDFs, streaming AEAD, or persistent private-key stores
in this API. Ed25519 keys can be read from PKCS #8 and SubjectPublicKeyInfo
with the [key import functions](crypto-keys.md#import). It supports the project's
x86-64 Linux GNU target. Runtime entry symbols keep unused primitive paths
eligible for linker garbage collection; `ring` is also used by `std.tls`.

## Acknowledgements

- Wraps [`ring`](https://github.com/briansmith/ring) 0.17.14 (Apache-2.0 AND ISC),
  already vendored for TLS, for all primitives on this page. Its complete
  license text and third-party notices are included by the release notice
  generator through the Cargo dependency graph.
- [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032) supplies the Ed25519 known-answer test.
- [RFC 5869](https://www.rfc-editor.org/rfc/rfc5869) supplies the HKDF-SHA256 known-answer test.
- [RFC 8439](https://www.rfc-editor.org/rfc/rfc8439) supplies the ChaCha20-Poly1305 known-answer test.
- The published [GCM test cases](https://csrc.nist.gov/projects/block-cipher-techniques/bcm/current-modes) supply the AES-256-GCM known-answer test.

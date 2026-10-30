# Authenticated encryption, signatures, and key agreement

[`std.crypto`](crypto.md) provides AES-256-GCM, ChaCha20-Poly1305,
Ed25519, ephemeral X25519, and HKDF-SHA256 over [`Bytes`](bytes.md).
> **Upstream code, used as-is.** These are unmodified `ring` primitives.
> Plenty implements argument validation, buffer ownership and error handling,
> and no cryptography: the algorithms and their side-channel resistance are
> deliberately left to `ring`. See [`std.crypto`](crypto.md) for this policy.

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
| `ed25519_verify(public_key, message, signature)` | `True` for a valid signature; `False` for a mismatch |

Every input is `&Bytes`; `target` is `&mut Bytes`. A seed must be exactly
32 cryptographically random bytes. Generate one with `std.secrets.token_bytes(32)`;
its public key may be shared, but its seed must remain secret. Signing is
deterministic, and a signing seed can sign multiple messages. Verification
requires exactly 32 public-key bytes and 64 signature bytes; other lengths
return `InvalidLength`, whereas correctly sized malformed keys/signatures
return `False`. This is plain Ed25519, without a prehash or context mode.
Public keys must be authenticated by the application's protocol. Small-order
and mixed-order public keys are not rejected. With such a key, a signature can
verify without any matching seed; with the identity point, one fixed signature
verifies for every message. Never accept a public key that the protocol has
not authenticated. The
[Wycheproof vectors](crypto-keys.md#test-vectors) record this as their
`acceptable` cases.

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
| `Allocation(AllocError)` | A fallible buffer or private-state allocation failed |

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

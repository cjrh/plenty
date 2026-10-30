# RSA and ECDSA signatures and key import

[`std.crypto`](crypto.md) signs and verifies with RSA (PKCS #1 v1.5 and PSS)
and ECDSA over P-256, and imports public and private keys from the standard
DER encodings, JWK components, and (through [`std.encoding`](encoding-codecs.md#pem))
PEM. These are the signature schemes of JOSE and JWT: RS256, PS256 and ES256,
with the SHA-384 and SHA-512 RSA variants. Ed25519 keys import here too and
sign with the [Ed25519 functions](crypto-advanced.md#ed25519).

> **Upstream code, used as-is.** Signing, verification and private-key
> parsing are unmodified `ring` primitives; Plenty implements no cryptographic
> arithmetic. Plenty parses public-key encodings and checks RSA key sizes
> itself, in checked code, so a public key reaches `ring` only after its
> structure is validated. See [`std.crypto`](crypto.md) for this policy.
Every function returns `Result[_, AdvancedError]`. See the
[signature index](signatures.md#stdcrypto) for complete declarations.

```plenty
import std.bytes
import std.crypto
import std.encoding
from std.bytes import BytesError
from std.crypto import AdvancedError
from std.encoding import EncodingError

def main() -> Result[(), IoError | BytesError | AdvancedError | EncodingError]:
    # The ES256 example from RFC 7515 appendix A.3: a JWK's x and y, the
    # signing input, and its fixed 64-byte signature.
    x = std.encoding.base64url_from_text("f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU")?
    y = std.encoding.base64url_from_text("x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0")?
    key = std.crypto.p256_public_key_from_coordinates(&x, &y)?
    mut message = std.bytes.from_text("eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ")?
    signature = std.encoding.base64url_from_text("DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1Q")?
    print(key.verify(&message, &signature)?)?

    message.set(0, 0u8)?
    print(key.verify(&message, &signature)?)?
    Ok(())
```
```output
True
False
```

## Key types

Each key type is an opaque class for one algorithm. There is no conversion
between them, no way to read a private key's material back out, and no way to
use one as an HMAC key or Ed25519 seed. The constructors are private: a key
exists only after an import function validated it. Keys cannot be copied.
Private keys are [redacted](../language/03-types/classes.md#redacted-types): printing one shows
`std.crypto.RsaPrivateKey(???)` or `std.crypto.P256PrivateKey(???)`.

| Type | Holds | Methods |
|---|---|---|
| `RsaPublicKey` | A 2048- to 8192-bit modulus and its exponent | `signature_size()`, `verify(scheme, message, signature)` |
| `RsaPrivateKey` | A 2048- to 4096-bit two-prime key, held by the runtime | `signature_size()`, `public_key()`, `sign(scheme, message)`, `sign_into(scheme, message, target)` |
| `P256PublicKey` | A point on P-256 | `verify(message, signature)` |
| `P256PrivateKey` | A P-256 scalar and its public point, held by the runtime | `public_key()`, `sign(message)`, `sign_into(message, target)` |

`RsaScheme` selects the RSA padding and hash for each call:

| `RsaScheme` | JOSE `alg` | Padding |
|---|---|---|
| `Pkcs1Sha256`, `Pkcs1Sha384`, `Pkcs1Sha512` | RS256, RS384, RS512 | PKCS #1 v1.5 ([RFC 8017 section 8.2](https://www.rfc-editor.org/rfc/rfc8017.html#section-8.2)) |
| `PssSha256`, `PssSha384`, `PssSha512` | PS256, PS384, PS512 | PSS with MGF1 over the same hash and a salt of the hash length ([RFC 7518 section 3.5](https://www.rfc-editor.org/rfc/rfc7518.html#section-3.5)) |

P-256 keys use ECDSA with SHA-256 only (JOSE ES256). Choose the scheme from
the key's configured algorithm, never from an unauthenticated message header.

## Import

| Function | Input | Result |
|---|---|---|
| `rsa_public_key_from_spki(der)` | SubjectPublicKeyInfo with `rsaEncryption` and NULL parameters | `RsaPublicKey` |
| `rsa_public_key_from_components(n, e)` | Big-endian modulus and exponent, as a JWK's decoded `n` and `e` | `RsaPublicKey` |
| `rsa_private_key_from_pkcs8(der)` | Unencrypted PKCS #8 v1 holding an RSAPrivateKey | `RsaPrivateKey` |
| `p256_public_key_from_spki(der)` | SubjectPublicKeyInfo with `id-ecPublicKey` and the named curve `prime256v1` | `P256PublicKey` |
| `p256_public_key_from_point(point)` | A 65-byte uncompressed SEC1 point: `0x04`, x, y | `P256PublicKey` |
| `p256_public_key_from_coordinates(x, y)` | 32-byte big-endian coordinates, as a JWK's decoded `x` and `y` | `P256PublicKey` |
| `p256_private_key_from_pkcs8(der)` | Unencrypted PKCS #8 v1 holding an ECPrivateKey with its public key | `P256PrivateKey` |
| `ed25519_public_key_from_spki(der)` | SubjectPublicKeyInfo with `id-Ed25519`; a [small-order key](crypto-advanced.md#small-order-public-keys) is `SmallOrderKey` | The 32-byte public key, as `Bytes` |
| `ed25519_seed_from_pkcs8(der)` | Unencrypted PKCS #8 v1 or v2 holding an Ed25519 private key | The 32-byte seed, as `Bytes` |

Every input is `&Bytes`. These are the forms `openssl genpkey` and
`openssl pkey -pubout` write. For PEM, decode with
`std.encoding.pem_decode(source, start, stop, b"PUBLIC KEY")` or
`b"PRIVATE KEY"` first; the label must match. Encrypted PKCS #8, the
PKCS #1 `RSA PRIVATE KEY` and SEC1 `EC PRIVATE KEY` forms, and X.509
certificates are not accepted.

DER is parsed strictly: lengths must be definite and minimal, integers minimal
and positive, and nothing may follow the outer element. The algorithm
identifier must be exactly the expected encoding, so another algorithm,
another curve, explicit curve parameters, or RSA-PSS-restricted keys
(`id-RSASSA-PSS`) are rejected. Public points must be uncompressed and on the
curve. RSA public keys must have an odd modulus of 2048 to 8192 bits without
a leading zero byte and an odd exponent from 3 to 2<sup>33</sup> - 1; private
keys are limited by `ring` to 2048 to 4096 bits, an exponent of at least
65537, and two primes, and their components must be consistent. A P-256
PKCS #8 key must include its public key, which must match the scalar.
An Ed25519 v2 key may include its public key, which must match the seed.

Validating a public point runs one ephemeral P-256 key agreement, because
`ring` has no separate point check; it needs operating-system randomness and
can fail with `RandomFailure`.

## Signing and verification

`verify` returns `True` for a valid signature and `False` for any signature
that does not verify, including malformed values of the right length. Only
misuse is an error: a signature whose length is wrong for the key is
`InvalidLength`. An RSA signature is exactly `signature_size()` bytes, the
modulus length. An ECDSA signature is the fixed 64-byte `r || s` form that JOSE
uses ([RFC 7518 section 3.4](https://www.rfc-editor.org/rfc/rfc7518.html#section-3.4)),
so a DER-encoded ECDSA signature never verifies; it is `InvalidLength`.

`sign` returns a new buffer; `sign_into` writes a target of exactly the
signature size and otherwise returns `InvalidLength` without writing.
PKCS #1 v1.5 signatures are deterministic. PSS draws a random salt and ECDSA
a random nonce (hedged by `ring` with the key and message), so repeated
signatures differ, and both can fail with `RandomFailure`. A PSS failure zeros
the target.

## Errors

| `AdvancedError` | Meaning here |
|---|---|
| `InvalidKey` | A key encoding is malformed, uses another algorithm or curve, or breaks a size or consistency rule other than the minimum |
| `WeakKey` | An RSA modulus is shorter than 2048 bits |
| `SmallOrderKey` | An Ed25519 public key has [small order](crypto-advanced.md#small-order-public-keys) |
| `InvalidLength` | A signature or signing target has the wrong length for the key |
| `RandomFailure` | Operating-system randomness failed during point validation, PSS or ECDSA signing, or private-key import |
| `Allocation(AllocError)` | A fallible buffer or key-state allocation failed, or memory for an RSA operation could not be reserved |

These extend the existing `AdvancedError` rather than adding a type, because
the operations are the same family as Ed25519 and share its length, entropy
and allocation failures; one error type keeps a JWT verifier's error set small.
`InvalidKey` and `WeakKey` carry no payload: no offset, field name or value,
so an error message cannot reveal anything about a private key.

## Allocation and key material

Each import allocates its result fallibly: a public key's validated DER or
point, a private key's runtime state object, or a returned Ed25519 key or seed.
`rsa_public_key_from_components` builds the DER from `n` and `e`. P-256
signing into an existing target and P-256 verification allocate nothing;
tests enforce this with the runtime's allocation guard, as they do for
`ring`'s P-256 private-key parsing and point validation.

`ring`'s RSA operations allocate big-number buffers through the Rust global
allocator, which cannot report failure. Before each one the runtime reserves a
fixed block, then runs `ring` with that thread's allocations taken from it:

| Operation | Reserved | When | Released |
|---|---|---|---|
| `rsa_private_key_from_pkcs8` | 12 KiB | before parsing | when the key is dropped; at once if parsing fails |
| `RsaPrivateKey.sign`, `sign_into` | 8 KiB | each call | when the call returns |
| `RsaPublicKey.verify` | 12 KiB | each call | when the call returns |

`signature_size` and `public_key` reserve nothing; `public_key` allocates only
its result. Each size is at least twice the most that `ring` 0.17.14 uses for
the largest key it accepts: a 4096-bit private modulus or an 8192-bit public
one. `ring` makes a fixed number of buffers per operation, each at most
1 KiB, so the use does not depend on the message or the key's value. A test
measures it and fails if a `ring` update outgrows a block.

If the block cannot be reserved, or `ring` asks for more than it holds, the
call returns `Allocation`. A failure inside `ring` abandons its frames, which
hold only the block and the stack, so nothing else needs repair; the block
is then released. The key, the target and the program stay usable; a failed
signature leaves the target zeroed. The blocks come from the current heap: in
an actor handler they count against the actor's
[heap limit](actor.md#memory-accounting).

A private key's runtime state is overwritten with volatile zero writes when it
is dropped. For P-256 that covers the scalar and nonce key, which `ring` keeps
inline. For RSA, the key's reserved block, which holds the primes, exponents
and coefficients, is zeroed before it is freed, as is each call's block. A
key may be used, and dropped, on any thread. Not scrubbed:

- the caller's DER or PEM input buffers, and the intermediate base64 and
  decoded buffers that `std.encoding.pem_decode` releases;
- the seed returned by `ed25519_seed_from_pkcs8`, which belongs to the caller
  (on rejection, the function zeros its own copy);
- copies `ring` makes in registers or on the stack while it runs.

`Bytes` does not zero its storage on drop; see
[key material](crypto.md#key-material).

## Test vectors

Every `valid` and `invalid` case in these
[Wycheproof](https://github.com/C2SP/wycheproof) files gives the expected
result, with the key imported through each available form (SPKI, JWK
members, and the raw point or key):

| File | Cases | Result |
|---|---|---|
| `rsa_signature_2048_sha256`, `_2048_sha384`, `_2048_sha512`, `_3072_sha256`, `_4096_sha512` | 258 to 259 each | All pass |
| `rsa_pss_2048_sha256_mgf1_32`, `_2048_sha384_mgf1_48`, `_4096_sha512_mgf1_64` | 108, 141, 179 | All pass |
| `ecdsa_secp256r1_sha256_p1363` | 262 | All pass |
| `ecdsa_secp256r1_sha256` (DER signatures) | 484 | None verifies: each is `InvalidLength` |
| `ed25519` | 210 | All pass |

Cases marked `acceptable` are pinned to the observed behavior:

- The one PKCS #1 v1.5 case per file whose DigestInfo omits the NULL
  parameters (`MissingNull`) is rejected.
- The 8 Ed25519 cases with small-order public keys (tcId 152 to 159) are
  `SmallOrderKey`, from SPKI import and from verification. The 21 with
  mixed-order keys (160 to 180) verify. See
  [small-order public keys](crypto-advanced.md#small-order-public-keys).

The extracts and their generator, with the upstream commit and file digests,
are in `tests/fixtures/crypto/wycheproof`.

## Limits

- No key generation for RSA or P-256, and no private-key export.
- No P-384, P-521, secp256k1, or ECDSA with DER signatures.
- No encrypted PKCS #8, PKCS #1 or SEC1 private-key forms, or certificates.
- No JWK parsing: decode the JSON and base64url members and pass the bytes.
- No RSA encryption.
- One target, x86-64 Linux GNU.

## Acknowledgements

- Wraps [`ring`](https://github.com/briansmith/ring) 0.17.14 (Apache-2.0 AND ISC),
  already vendored for TLS, for RSA and ECDSA signing, verification, and
  private-key parsing.
- Project Wycheproof (C2SP/wycheproof, Apache-2.0) supplies the RSA, ECDSA
  and Ed25519 [test vectors](#test-vectors).
- [RFC 7515](https://www.rfc-editor.org/rfc/rfc7515) appendices A.2 and A.3
  supply the RS256 and ES256 known-answer tests, and
  [RFC 8037](https://www.rfc-editor.org/rfc/rfc8037) appendix A the Ed25519 one.
- Encodings follow [RFC 5280](https://www.rfc-editor.org/rfc/rfc5280)
  (SubjectPublicKeyInfo), [RFC 5208](https://www.rfc-editor.org/rfc/rfc5208)
  and [RFC 5958](https://www.rfc-editor.org/rfc/rfc5958) (PKCS #8),
  [RFC 5480](https://www.rfc-editor.org/rfc/rfc5480) and
  [RFC 5915](https://www.rfc-editor.org/rfc/rfc5915) (elliptic-curve keys),
  [RFC 8410](https://www.rfc-editor.org/rfc/rfc8410) (Ed25519 keys), and
  [RFC 8017](https://www.rfc-editor.org/rfc/rfc8017) (RSA).

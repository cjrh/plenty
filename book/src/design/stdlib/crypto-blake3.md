# BLAKE3 hashing and key derivation

[`std.crypto`](crypto.md) provides BLAKE3's three native modes. Each produces
32 bytes and accepts `std.bytes.Bytes` input. The incremental forms use the
same `Digest` and `Mac` methods and range rules as SHA and HMAC.

| Function | Result | Contract |
|---|---|---|
| `blake3(data: &Bytes)` | `Result[Bytes, CryptoError]` | Unkeyed hash |
| `digest(Algorithm.Blake3)` | `Result[Digest, CryptoError]` | Incremental unkeyed hash |
| `blake3_keyed(key: &Bytes, data: &Bytes)` | `Result[Bytes, CryptoError]` | Native keyed hash; exactly 32 key bytes |
| `blake3_keyed_hasher(key: &Bytes)` | `Result[Mac, CryptoError]` | Incremental keyed hash, including constant-time `verify` |
| `blake3_derive_key(context: &str, key_material: &Bytes)` | `Result[Bytes, CryptoError]` | Derive a key from high-entropy material under a context |
| `blake3_derive_hasher(context: &str)` | `Result[Digest, CryptoError]` | Incremental key derivation |

`output_size(Algorithm.Blake3)` is 32. A keyed constructor rejects any other
key length with `CryptoError.InvalidKeyLength`. Generate keys with
[`std.secrets.token_bytes(32)`](secrets.md). This keyed mode is BLAKE3's own
construction; `hmac(Algorithm.Blake3, key)` returns
`CryptoError.UnsupportedAlgorithm`.

For derivation, choose a fixed, globally unique, application-specific context
that identifies the purpose, such as `"example.org messenger 2026-10-27 session key"`.
The context is public UTF-8 text, not a salt, password or secret key. Different
purposes should use different contexts. The key material must already have
sufficient entropy: this API is not password hashing or password stretching.

## State, allocation and implementation

Each constructor makes one fallible state allocation. BLAKE3 has a larger
fixed-size inline tree stack than SHA; it uses a separate object layout, so
adding this algorithm does not increase SHA/HMAC state allocations. Context
hashing in the derivation constructor needs no extra allocation.

`update`, `update_text`, `finish_into` and `Mac.verify` allocate nothing,
including when input crosses tree-chunk boundaries. `finish` allocates an
output buffer; one-shot functions allocate a state and output buffer. An
output allocation failure leaves the state unfinished and retryable. Invalid
ranges also leave it unchanged. A successful finish or a verification
(including a mismatch) ends the state.

The runtime wraps the BLAKE3 team's `blake3` crate 1.8.7 rather than
implementing a cryptographic primitive. Both runtime builds use its `pure`
feature without `std` or Rayon: Rust SIMD implementations select SSE2,
SSE4.1 or AVX2 at runtime on supported processors, with a portable fallback.
There are no worker threads, assembly or AVX-512 paths in this configuration.
Only the standard 32-byte output is exposed; extendable output is not exposed.

Finishing or dropping a BLAKE3 state overwrites the stored state using
volatile writes. This includes keyed and derivation modes. Caller-owned keys,
input buffers, result buffers and temporary upstream stack copies are not
scrubbed; see [key material](crypto.md#key-material).

The unmodified sources and licenses are vendored alongside `arrayvec` 0.7.8,
`constant_time_eq` 0.4.2 and `cpufeatures` 0.3.1. Release notices include these
Cargo dependencies. Tests use the upstream 1.8.7 known-answer vectors for
all three modes and allocation-failure instrumentation for the Plenty API.

## Acknowledgements

- Wraps the BLAKE3 team's [`blake3`](https://github.com/BLAKE3-team/BLAKE3) 1.8.7 implementation and uses its published test vectors (CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception); code and licenses are vendored in the runtime.
- BLAKE3's vendored dependencies are [`arrayvec`](https://github.com/bluss/arrayvec) 0.7.8 for its inline tree stack (MIT OR Apache-2.0), [`constant_time_eq`](https://github.com/cesarb/constant_time_eq) 0.4.2 for comparison support (CC0-1.0 OR MIT-0 OR Apache-2.0), and RustCrypto's [`cpufeatures`](https://github.com/RustCrypto/utils) 0.3.1 for processor detection (MIT OR Apache-2.0).

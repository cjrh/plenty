# std.crypto

`import std.crypto` computes BLAKE3, SHA-256, SHA-512 and legacy SHA-1 digests and HMAC tags over
[`std.bytes`](bytes.md) buffers, and compares byte buffers in constant time.
It has one error type, `CryptoError`.

Primitives use third-party implementations: the RustCrypto `sha2`, `sha1`,
`hmac`, `subtle`, and the BLAKE3 team's `blake3` crate, compiled into the runtime. The Plenty module validates
arguments and reports errors; it implements no cryptography. The module is
available on the supported x86-64 Linux GNU target.

## Functions

See [BLAKE3 hashing and key derivation](crypto-blake3.md) for its one-shot,
incremental keyed-hash and derivation functions.

| Function | Result | Contract |
|---|---|---|
| `sha256(data: &Bytes)` | `Result[Bytes, CryptoError]` | The 32-byte SHA-256 digest of `data` |
| `sha512(data: &Bytes)` | `Result[Bytes, CryptoError]` | The 64-byte SHA-512 digest of `data` |
| `sha1(data: &Bytes)` | `Result[Bytes, CryptoError]` | The 20-byte SHA-1 digest of `data`; legacy, see below |
| `hmac_sha256(key: &Bytes, data: &Bytes)` | `Result[Bytes, CryptoError]` | The 32-byte HMAC-SHA-256 tag |
| `hmac_sha512(key: &Bytes, data: &Bytes)` | `Result[Bytes, CryptoError]` | The 64-byte HMAC-SHA-512 tag |
| `digest(algorithm: Algorithm)` | `Result[Digest, CryptoError]` | A new incremental digest |
| `hmac(algorithm: Algorithm, key: &Bytes)` | `Result[Mac, CryptoError]` | A new incremental HMAC under `key` |
| `equal(a: &Bytes, b: &Bytes)` | `bool` | Whether `a` and `b` hold the same bytes, in constant time |
| `output_size(algorithm: Algorithm)` | `i64` | 32 for `Sha256` and `Blake3`, 64 for `Sha512`, 20 for `Sha1` |

`Algorithm` is `Sha256`, `Sha512`, `Sha1` or `Blake3`. HMAC supports the SHA
algorithms; BLAKE3 has its own keyed mode. HMAC follows RFC 2104 and accepts a key
of any length, including empty; a key longer than the hash block (64 bytes for
SHA-256 and SHA-1, 128 for SHA-512) is hashed first. Choose a key of at least the output
size from [`std.secrets`](secrets.md).

SHA-1 is broken for collision resistance: an attacker can make two inputs
with the same digest. It is here only for protocols that fix it, such as the
[WebSocket handshake](websocket.md) and Git object names. Do not use it for
signatures, integrity checks against an adversary, or new designs.

## Incremental states

`Digest` and `Mac` have the same methods. Feeding input in any number of
pieces gives the same output as one call over the joined input.

| Method | Result | Contract |
|---|---|---|
| `size()` | `i64` | The output size in bytes |
| `update(data: &Bytes, start: i64, stop: i64)` | `Result[(), CryptoError]` | Absorb `data[start:stop]` |
| `update_text(text: &str)` | `Result[(), CryptoError]` | Absorb the UTF-8 bytes of `text` |
| `finish()` | `Result[Bytes, CryptoError]` | The output in a new buffer |
| `finish_into(target: &mut Bytes, offset: i64)` | `Result[(), CryptoError]` | Write the output at `target[offset:offset + size()]` |
| `Mac.verify(expected: &Bytes, start: i64, stop: i64)` | `Result[bool, CryptoError]` | Finish, and compare the tag with `expected[start:stop]` in constant time |

`update`, `finish_into`, and `verify` take their range under the
[range rule](bytes.md#the-range-rule). Each state finishes once: `finish`,
`finish_into`, and `verify` end it, and every later call is `Finished`. When
`finish` cannot allocate its result, the state is left unfinished and the call
may be retried. A tag whose length differs from `size()`, such as a truncated
tag, does not verify; `verify` does not accept truncation.

The fields are private, so a state can only be made by the module's constructors.
Printing one shows its size, never its contents.

## Errors

```text
pub enum CryptoError:
    OutOfRange
    Finished
    InvalidKeyLength
    UnsupportedAlgorithm
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `OutOfRange` | A range outside its buffer, or an output that does not fit at `offset`; the state is unchanged |
| `Finished` | The state has already produced its output |
| `InvalidKeyLength` | A BLAKE3 key does not contain exactly 32 bytes |
| `UnsupportedAlgorithm` | `hmac` was called with `Algorithm.Blake3`; use native keyed BLAKE3 |
| `Allocation(cause)` | A state or output buffer could not be allocated |

## Constant-time comparison

`equal` and `Mac.verify` use `subtle::ConstantTimeEq` (through
`Mac::verify_slice` for HMAC). Their running time depends on the lengths of
the operands, not on their contents or where they first differ. Lengths are
not secret: comparing buffers of different lengths returns `False` at once.
`Digest` has no `verify`; compare a finished digest with `equal`.

`==` on `Bytes` stops at the first difference. Never use it, or a loop of
`get` calls, to check a tag, token, or password hash: its timing tells an
attacker how many leading bytes of a guess are right.

The guarantee is that of `subtle` 2.6: comparisons combine bytes without
branches and pass the result through an optimisation barrier. It is a
best-effort property of compiled code, not a hardware guarantee, and it covers
only the comparison, not how the program reacts to the answer.

## Key material

A `Mac` holds its key's padded forms or BLAKE3 key state. Finishing it and dropping it overwrite
that state with volatile writes before the memory is released. Plenty does not
scrub the rest:

- The key `Bytes` passed to `hmac` belongs to the caller, who must protect it;
  Plenty does not zero memory on drop.
- Outputs from `finish` are ordinary buffers.
- Copies the hashing code makes on the stack while it runs are not cleared.
- `update_text` reads a `str`, which is never scrubbed.

## Ownership and threads

A state owns one runtime object and nothing else; it holds no descriptor and
needs no closing. Inputs are shared loans and `finish_into`'s target is an
exclusive loan. States cannot be copied. The module has no global state,
passes the ordinary worker-effect checks, and may be used from any thread;
moving a state to a worker moves its ownership.

`update` and `finish_into` take buffers, not states, as operands. The
back-end methods `crypto_update`, `crypto_finish`, and their siblings are
public on `Bytes` only so that `std.crypto` can call them. On a buffer not
made by `std.crypto` they stop the program with
`operand is not a std.crypto state`.

## Costs

| Operation | Allocates | Time |
|---|---|---|
| SHA `digest`, `hmac` | One state object of 368 bytes plus the allocation header | Constant, plus one hash of the key when it exceeds a block |
| `update`, `update_text` | Nothing | Linear in the input |
| `finish_into`, `verify` | Nothing | Final blocks; BLAKE3 also folds its bounded tree stack |
| `finish` | One output buffer | As `finish_into` |
| `sha256` and the other one-shots | A state and the output | Linear in the input |
| `equal` | Nothing | Linear in the length; buffers of different lengths return at once |

Input is never copied: `update` hashes directly from the buffer's storage.
For BLAKE3 state and finalization costs, see [its allocation contract](crypto-blake3.md#state-allocation-and-implementation).
SHA-256 uses the x86-64 SHA extensions and SHA-512 uses AVX2 when the
processor reports them at run time, and portable code otherwise; the result is
the same.

## Supply

The runtime builds these crates from unmodified copies under
`plenty-runtime/vendor`, so building the compiler needs no network and an
installed compiler needs neither the sources nor a Rust toolchain:

| Crate | Version | Licence |
|---|---|---|
| `sha2` | 0.10.9 | MIT or Apache-2.0 |
| `sha1` | 0.10.6 | MIT or Apache-2.0 |
| `hmac` | 0.12.1 | MIT or Apache-2.0 |
| `subtle` | 2.6.1 | BSD-3-Clause |
| `digest`, `block-buffer`, `crypto-common` | 0.10.7, 0.10.4, 0.1.7 | MIT or Apache-2.0 |
| `generic-array`, `typenum` | 0.14.7, 1.20.1 | MIT; MIT or Apache-2.0 |
| `cpufeatures`, `cfg-if` | 0.2.17, 1.0.4 | MIT or Apache-2.0 |

Each crate's licence file is kept with its sources. The same versions are
pinned in `Cargo.lock`, and a test fails if the two disagree. To update one,
replace its directory with the new release, run `cargo update -p CRATE
--precise VERSION`, and rerun the known-answer tests in
`tests/test_std_crypto.rs`.

Executables are linked with `--gc-sections`, and the hashing code is reached
only through its own runtime entry symbols, so a program that does not use
`std.crypto` contains none of it. The `link-args.txt` written by
`--emit-runtime` includes that flag.

## Limits

- No encryption, authenticated encryption, signatures, or key exchange.
- Key derivation currently uses BLAKE3 only; no HKDF, PBKDF2, Argon2 or password hashing.
- No MD5, SHA-3, BLAKE2, or truncated SHA-512 variants.
- No zero-on-drop buffer type; see [Key material](#key-material).
- One target, x86-64 Linux GNU.

## Acknowledgements

- Wraps the BLAKE3 team's [`blake3`](https://github.com/BLAKE3-team/BLAKE3) 1.8.7 (CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception), with [dependency credits](crypto-blake3.md#acknowledgements).

- Wraps the RustCrypto [`sha2`](https://github.com/RustCrypto/hashes) 0.10.9, [`sha1`](https://github.com/RustCrypto/hashes) 0.10.6 and [`hmac`](https://github.com/RustCrypto/MACs) 0.12.1 crates (MIT OR Apache-2.0) and [`subtle`](https://github.com/dalek-cryptography/subtle) 2.6.1 (BSD-3-Clause), vendored in the runtime.

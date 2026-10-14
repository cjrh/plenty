# std.encoding

`import std.encoding` provides hex and base64 codecs and fixed-width integer
packing over [`std.bytes`](bytes.md) buffers, with one error type,
`EncodingError`.

The module is checked Plenty code with no native primitives of its own: every
byte is read and written through the validated `Bytes` operations. It is
available on the supported x86-64 Linux GNU target.

These are binary encodings. They do not normalize, transcode, or validate text;
turning decoded bytes into a `str` is the separate, strict
[`Bytes.to_text`](bytes.md) conversion.

## Codecs

`source` is `&Bytes`; `start` and `stop` are `i64` and follow the
[range rule](bytes.md#the-range-rule) `0 <= start <= stop <= len`.

| Function | Result | Contract |
|---|---|---|
| `hex_encode(source, start, stop)` | `Result[Bytes, EncodingError]` | Two lowercase digits per byte |
| `hex_decode(source, start, stop)` | `Result[Bytes, EncodingError]` | Digits of either case; even length |
| `base64_encode(source, start, stop)` | `Result[Bytes, EncodingError]` | RFC 4648 section 4, padded |
| `base64_decode(source, start, stop)` | `Result[Bytes, EncodingError]` | The padded form only |
| `base64url_encode(source, start, stop)` | `Result[Bytes, EncodingError]` | RFC 4648 section 5, unpadded |
| `base64url_decode(source, start, stop)` | `Result[Bytes, EncodingError]` | The unpadded form only |
| `hex_text`, `base64_text`, `base64url_text` `(source, start, stop)` | `Result[str, EncodingError]` | The same encoding as a `str` |
| `hex_from_text`, `base64_from_text`, `base64url_from_text` `(text: &str)` | `Result[Bytes, EncodingError]` | Decode the whole text |

Each result is a new buffer whose capacity equals its length. Encoders produce
ASCII only.

### Accepted forms

Each codec accepts one form. Decoders never skip, trim, or repair input:
whitespace, line breaks, and any other byte outside the alphabet are errors.

| Codec | Alphabet | Length | Padding |
|---|---|---|---|
| hex | `0-9`, `a-f`, `A-F`; encoder writes lowercase | Even | None |
| base64 | `A-Z`, `a-z`, `0-9`, `+`, `/` | Multiple of 4 | `=` required to complete the last group |
| base64url | `A-Z`, `a-z`, `0-9`, `-`, `_` | Any except 1 more than a multiple of 4 | `=` rejected |

Base64 text must be canonical: bits of the last symbol that encode no byte must
be zero, so each byte sequence has exactly one accepted text per codec. The
empty range encodes and decodes to an empty buffer.

Decoder checks, in order:

1. The range, then the length.
2. For base64, up to two trailing `=` are set aside as padding.
3. Bytes are examined in order. The first one outside the alphabet is
   `InvalidPadding` if it is `=` and `InvalidByte` otherwise.
4. The trailing bits of a final partial group.

## Integer packing

`value` has the type in the function name; `offset` is `i64`; `order` is
`Endian.Little` or `Endian.Big`. There is no default or native order.

| Function | Result | Contract |
|---|---|---|
| `pack_u16`, `pack_u32`, `pack_u64`, `pack_i16`, `pack_i32`, `pack_i64` `(target: &mut Bytes, offset, value, order)` | `Result[(), EncodingError]` | Overwrite the bytes at `offset`; never grows |
| `append_u16`, `append_u32`, `append_u64`, `append_i16`, `append_i32`, `append_i64` `(target: &mut Bytes, value, order)` | `Result[(), EncodingError]` | Grow by exactly the width |
| `unpack_u16`, `unpack_u32`, `unpack_u64`, `unpack_i16`, `unpack_i32`, `unpack_i64` `(source: &Bytes, offset, order)` | `Result[u16, EncodingError]`, and so on | Read the bytes at `offset` |

`Endian.Big` puts the most significant byte at the lowest offset. Signed values
use two's complement. `pack_` and `unpack_` require
`0 <= offset <= len - width`; a short buffer is `OutOfRange` and nothing is
read or written. Bytes are accessed one at a time, so no offset has an
alignment requirement. A single byte needs no order: use `Bytes.get`, `set`,
and `push`.

## Errors

```text
pub enum EncodingError:
    OutOfRange
    InvalidLength
    InvalidByte(i64)
    InvalidPadding(i64)
    NonCanonical(i64)
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `OutOfRange` | A range or offset lies outside the buffer |
| `InvalidLength` | The encoded length is impossible for the codec |
| `InvalidByte(index)` | The byte at `index` is outside the alphabet |
| `InvalidPadding(index)` | `=` at `index` where the codec does not accept it |
| `NonCanonical(index)` | The symbol at `index` has nonzero bits that encode no byte |
| `Allocation(cause)` | Storage for the result could not be allocated |

An index is a position in the source buffer, not relative to `start`; for the
`_from_text` forms it is a UTF-8 byte offset into the text. Every fallible
operation returns `Result[T, EncodingError]`. A failed operation leaves its
`target` unchanged and releases anything it allocated. Wrap a result with
[`std.error.context`](error.md) to name the operation.

## Ownership and threads

Sources are shared loans and targets exclusive loans, so the
[view rules](bytes.md#views) apply: a buffer cannot be packed into while a
`&Bytes` to it is live, and cannot be encoded into itself. The module holds no
global state. Its functions pass the ordinary worker-effect checks and may run
in any thread.

## Costs

| Operation | Allocates | Time |
|---|---|---|
| `_encode`, `_decode` | One buffer of exactly the output size, after the range and length checks | Linear in the range |
| `_text` | The encoded buffer, then a `str` copy of it | Linear |
| `_from_text` | A `Bytes` copy of the text, then the decoded buffer | Linear |
| `append_` | Only when capacity is exceeded, as `Bytes.resize` | Constant, amortized |
| `pack_`, `unpack_` | Nothing | Constant |

Output sizes for `n` input bytes: hex `2n`; base64 `4 * ceil(n / 3)`;
base64url `ceil(4n / 3)`. A decoder allocates its output before it examines
the alphabet, so rejected input pays for that buffer, which is released before
returning.

Every byte goes through a checked single-element `Bytes` call. Measured with a
release compiler on x86-64 Linux, pinned to one core, on an 8,388,608-byte
input:

| Operation | Wall time |
|---|---|
| `hex_encode` | 1.9 s |
| `hex_decode` of the 16,777,216 digits | 2.5 s |
| `base64_encode` | 1.3 s |
| `base64_decode` of the 11,184,812 symbols | 1.3 s |
| 1,048,576 `pack_u64` and 1,048,576 `unpack_u64` | 2.9 s |

That is a few megabytes per second. Bulk native primitives would be faster and
are not implemented.

## Limits

- No streaming or incremental codecs: input and output are whole buffers.
- No line-wrapped (MIME) base64, base32, or user-supplied alphabets.
- No `f32`/`f64` packing, variable-length integers, or 128-bit integers.
- No record-layout description: pack fields one call at a time.

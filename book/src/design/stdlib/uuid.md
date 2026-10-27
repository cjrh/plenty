# `std.uuid`

UUID values for records, requests and files, with random v4 and timestamp-prefixed
v7 generation. The implementation is checked Plenty using
[`std.bytes`](bytes.md), [`std.secrets`](secrets.md) and [`std.time`](time.md).
No extra native library is linked. Start with the
[tutorial](../../tutorial/07-modules/20-standard-uuid.md); the
[signature index](signatures.md#stduuid) lists the complete API.

```plenty
import std.bytes
import std.secrets
import std.uuid

def main() -> Result[(), AllocError | IoError | ParseError | std.bytes.BytesError | std.secrets.SecretsError]:
    id = std.uuid.parse("6BA7B810-9DAD-11D1-80B4-00C04FD430C8")?
    print(id.to_string()?, id.version(), id.is_rfc_variant())?
    same = std.uuid.from_bytes(&id.to_bytes()?)?
    print(same == id)?
    fresh = std.uuid.v4()?
    print(len(fresh.to_string()?), fresh.version(), fresh == id)?
    Ok(())
```
```output
6ba7b810-9dad-11d1-80b4-00c04fd430c8 Option[u8].Some(1) True
True
36 Option[u8].Some(4) False
```

## Values and conversion

`Uuid` owns two private `u64` fields in network order: most significant half
first. It has ordinary class move semantics; `==` and `!=` compare the complete
128-bit value. All methods borrow their receiver.

| Operation | Result and contract |
| --- | --- |
| `parse(text: &str)` | `Result[Uuid, ParseError]`; exactly 36 ASCII characters in `8-4-4-4-12` hyphenated hex form, accepting either digit case |
| `from_bytes[S: ByteSource](source: &S)` | `Result[Uuid, ParseError]`; exactly 16 bytes, in big-endian/network order; accepts `Bytes` or a byte literal |
| `from_u64_pair(high: u64, low: u64)` | `Uuid`; preserves the supplied halves |
| `nil()` | `Uuid`; all zero bits |
| `id.to_string()` | `Result[str, BytesError]`; lowercase hyphenated text |
| `id.to_bytes()` | `Result[Bytes, BytesError]`; an owned 16-byte buffer |
| `id.write_bytes(target: &mut Bytes, offset: i64)` | `Result[(), BytesError]`; overwrites exactly 16 existing bytes; no growth or allocation |
| `id.as_u64_pair()` | `(u64, u64)`; a copyable, numerically ordered sorting key |
| `id.compare(other: &Uuid)` | `i64`; `-1`, `0`, or `1` in unsigned 128-bit order |
| `id.is_nil()` | `bool`; all bits are zero |
| `id.is_rfc_variant()` | `bool`; the two leading variant bits are `10` |
| `id.version()` | `Option[u8]`; the raw four-bit version for the RFC variant, otherwise `Nothing` |
| `id.unix_milliseconds()` | `Option[u64]`; the leading 48 bits for RFC-variant v7, otherwise `Nothing` |

Parsing and byte conversion preserve all bits, including nil, max, unrecognized
versions and non-RFC variants. Parsing does not validate generation history.
Malformed text and wrong byte lengths return `ParseError.Invalid`. There is no
trimming, URN/braced/compact spelling, little-endian GUID convention, or implicit
normalization. `write_bytes` validates the whole range before writing; invalid
offsets return `BytesError.OutOfRange` and leave the buffer unchanged.

Parsing, reading bytes, comparison, and numeric construction do not allocate.
`to_bytes` allocates its result; `to_string` uses a 36-byte scratch buffer and
allocates its result. Allocation failures are `BytesError.Allocation`.
Normal `print(id)` retains structural class formatting: use
`print(id.to_string()?)?` for canonical text. Class relational operators and
dictionary keys are not available; use `compare`, or `sort_by_key` with
`as_u64_pair()`. Use canonical text when a dictionary key is needed.

## Generation

```text
v4() -> Result[Uuid, SecretsError]
v7() -> Result[Uuid, RangeError | SecretsError]
```

`v4` obtains 16 OS-random bytes and sets the version and variant, leaving 122
random bits. `v7` reads `wall_now()` once and obtains 10 random bytes: its layout
is a 48-bit Unix millisecond timestamp and 74 random bits, plus version/variant.
Both propagate allocation and entropy errors from `std.secrets`; neither falls
back to a pseudorandom seed, and neither maintains a process-wide counter.

The clock value must have nonnegative seconds, nanoseconds below one billion,
and a millisecond value at most `281474976710655`. Sub-millisecond fractions
are rounded down; invalid timestamps return `RangeError.OutOfRange` before
requesting entropy. Generation uses a short-lived entropy buffer; a batch
generator with reusable scratch is not part of this API.

**v7 sorts by its recorded millisecond, not by call order.** Values in the same
millisecond have random order, and a backwards wall-clock adjustment can yield
a smaller UUID. There is no strict monotonicity or mathematical uniqueness
guarantee. The timestamp is visible to anyone reading the identifier. UUIDs are
identifiers, not authentication credentials; use `std.secrets` tokens for that
purpose.

For deterministic tests or an explicitly chosen entropy device:

```text
v4_from[R: Reader](source: &mut Source[R]) -> Result[Uuid, SecretsError]
v7_from[R: Reader](at: &Timestamp, source: &mut Source[R]) -> Result[Uuid, RangeError | SecretsError]
```

`Source` is `std.secrets.Source`, constructed with `source_from(reader)` or
`os()`. These functions apply the same masking and timestamp checks as the
convenience calls. Production callers must supply cryptographic entropy;
replaying a test source can repeat identifiers. Validation happens before any
read, and short/interrupted reads and failures follow the secrets source's
existing policy. No clock or entropy callback is stored in a UUID.

## References and scope

The layout and test vectors follow [RFC 9562](https://www.rfc-editor.org/rfc/rfc9562.html),
especially sections 5.4/5.7 and appendices A.3/A.6. API references are
[Python's `uuid`](https://docs.python.org/3/library/uuid.html) and
[Rust's `uuid::Uuid`](https://docs.rs/uuid/latest/uuid/struct.Uuid.html).
Plenty deliberately accepts fewer text spellings than either reference and
does not adopt Python's v7 counter or Rust's stateful `now_v7` ordering policy.
Name-based v3/v5 and v1/v6/v8 generators are not implemented.

## Acknowledgements

- API modelled on [Python's `uuid`](https://docs.python.org/3/library/uuid.html) and Rust's [`uuid::Uuid`](https://docs.rs/uuid/latest/uuid/struct.Uuid.html).

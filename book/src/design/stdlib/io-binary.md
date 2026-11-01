# Sequential binary fields in a Cursor

[`std.io.Cursor`](io.md) owns a `Bytes` buffer and one byte position. Its typed
methods use [`std.encoding`](encoding.md) to read and write fixed-width fields
without caller-maintained offsets. Raw `read`, `write`, and `seek` share this
same position; raw reads still allow short reads. No second cursor, stored
borrow, native serialization library, or independent position is involved.

## Scalar fields

`read_u8()` and `read_i8()` take no byte order. For each of `u16`, `i16`,
`u32`, `i32`, `u64`, `i64`, `f32`, and `f64`, `read_<type>(order: Endian)` selects explicit
little or big endian. The matching writes are
`write_<type>(value, order, max_bytes: i64 = 1048576)`; one-byte writes omit
`order`. There is no native-order default or alignment requirement. Signed
integers use two's complement. Floating fields use exact IEEE 754 binary32 or
binary64 bits, preserving signed zero, subnormals, infinities and NaN payloads
without arithmetic or width conversion, matching `std.encoding`.

All typed methods return `Result[..., BinaryError]`. A successful field
advances by its exact width. Failed fields leave position, contents, and length
unchanged. Earlier successful operations remain committed: this is per-field
atomicity, not record rollback. This guarantee is specific to memory; generic
stream helpers can report externally visible partial progress.

The byte order belongs to the operation, matching the underlying codecs and
making mixed-endian formats explicit without another mutable configuration.
Existing buffers are owned by `cursor(data)`; typed operations borrow the
cursor for one call. Mutating its public `data` changes what subsequent reads
see without changing the position.

## Payloads, bounds, and errors

- `remaining() -> i64` returns bytes after the current position, or zero after
  seeking past the end.
- `require_end() -> Result[(), BinaryError]` rejects remaining bytes without
  advancing. A position at or beyond the end succeeds.
- `read_bytes(count: i64, max_bytes: i64 = 1048576)` returns an owned `Bytes`
  copy. It checks the cap and available input before allocation. Returned
  borrowed views are not supported by the language. An empty read returns an
  empty owned buffer without moving, even after a seek past the end.
- `write_bytes(source: &[u8], max_bytes: i64 = 1048576)` borrows a literal,
  `Bytes`, list, or subview and writes its complete range. An empty write does
  not fill a seek gap or move position.

Every typed write's `max_bytes` bounds the final buffer length, including an
existing larger buffer and zero-filled seek gaps. Negative caps/counts and
position arithmetic overflow are `OutOfRange`; a valid size exceeding the cap
is `LimitExceeded`. Seek itself remains unrestricted except for its existing
nonnegative checked i64 position contract. Writing beyond the current end
fills the gap with zeros after successfully allocating the whole extent.

| `BinaryError` variant | Meaning |
|---|---|
| `Truncated(position, needed, remaining)` | The complete requested field is unavailable; all three values are byte counts/offsets |
| `TrailingData(position, remaining)` | `require_end` found unconsumed bytes |
| `OutOfRange` | Negative count/cap or position arithmetic overflow |
| `LimitExceeded` | Requested owned copy or final write buffer exceeds its cap |
| `Allocation(AllocError)` | Buffer creation/growth failed |
| `Encoding(EncodingError)` | Underlying codec cause, retained intact |

Scalar reads and writes within capacity allocate nothing. Growth uses `Bytes`
allocation, and owned payload reads allocate one buffer and copy `count` bytes.
Scalar operations are constant time; payload copies and zero-filled gaps take
time proportional to their size. The caps bound logical bytes, not allocator
metadata or resident memory. No synchronization or hidden worker restrictions
are added; the owning cursor can be moved to one worker as other owned buffers
can. Callers cannot borrow its data as a write source while borrowing the
cursor mutably.

See the [binary packet tutorial](../../tutorial/08-resources/binary-packets.md)
for a runnable example.

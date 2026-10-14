# std.bytes

`import std.bytes` provides `Bytes`, an owning contiguous buffer that stores one
byte per element, and `BytesError`. A `list[u8]` stores each element in a
general collection slot; `Bytes` exists for binary data large enough for that to
matter. Bytes are never text: converting to `str` is an explicit, validated
operation.

The module is embedded Plenty code over a private native storage owner. It is
available on the supported x86-64 Linux GNU target.

## Constructing

| Function | Result | Contract |
|---|---|---|
| `std.bytes.empty()` | `Result[Bytes, BytesError]` | Empty buffer with no reserved capacity |
| `std.bytes.with_capacity(capacity)` | `Result[Bytes, BytesError]` | Empty buffer with exactly `capacity` bytes reserved; negative is `OutOfRange` |
| `std.bytes.from_list(values: &list[u8])` | `Result[Bytes, BytesError]` | Independent copy of the list's bytes |
| `std.bytes.from_text(text: &str)` | `Result[Bytes, BytesError]` | Independent copy of the exact UTF-8 bytes |

`Bytes` has no public constructor and its storage field is private. Every
constructor allocates and can report `Allocation`.

## Operations

`start`, `stop`, `index`, `offset`, `count`, and sizes are `i64`; elements are `u8`.

| Method | Receiver | Result | Contract |
|---|---|---|---|
| `len()` | shared | `i64` | Number of bytes |
| `is_empty()` | shared | `bool` | Whether the length is zero |
| `capacity()` | shared | `i64` | Bytes storable without reallocating |
| `get(index)` | shared | `Result[u8, BytesError]` | Read one byte |
| `find(value, start, stop)` | shared | `Result[Option[i64], BytesError]` | First index of `value` within the range |
| `slice(start, stop)` | shared | `Result[Bytes, BytesError]` | Independent copy of a range, with exact capacity |
| `copy()` | shared | `Result[Bytes, BytesError]` | Independent copy of the whole buffer |
| `to_list()` | shared | `Result[list[u8], BytesError]` | Independent `list[u8]` for the transport builtins |
| `to_text()` | shared | `Result[str, BytesError]` | Strictly validated UTF-8 copy; otherwise `InvalidUtf8` |
| `set(index, value)` | exclusive | `Result[(), BytesError]` | Replace one byte |
| `write_float(value: f64)` | exclusive | `Result[i64, BytesError]` | Write shortest scientific ASCII into existing bytes at zero; requires `len() >= 32`, returns bytes written, never allocates or changes length |
| `fill(start, stop, value)` | exclusive | `Result[(), BytesError]` | Replace every byte of a range |
| `overwrite(offset, source, start, stop)` | exclusive | `Result[(), BytesError]` | Copy `source[start:stop]` over existing bytes at `offset`; never grows |
| `push(value)` | exclusive | `Result[(), BytesError]` | Append one byte |
| `extend(source)` | exclusive | `Result[(), BytesError]` | Append all of another buffer |
| `extend_range(source, start, stop)` | exclusive | `Result[(), BytesError]` | Append a range of another buffer |
| `reserve(additional)` | exclusive | `Result[(), BytesError]` | Ensure room for `additional` more bytes |
| `resize(count, fill)` | exclusive | `Result[(), BytesError]` | Set the length, appending `fill` or dropping the tail |
| `truncate(count)` | exclusive | `Result[(), BytesError]` | Shorten to `count`; longer than the length is `OutOfRange` |
| `pop()` | exclusive | `Option[u8]` | Remove and return the last byte, or `Nothing` |
| `clear()` | exclusive | `()` | Set the length to zero, keeping capacity |

`source` parameters are `&Bytes`. `==` and `!=` compare contents. Printing a
buffer shows printable ASCII and writes every other byte as `\xhh`; it is a
diagnostic form, not a conversion.

`Bytes` is a resource: assignment and argument passing move it, and the builtin
`copy` rejects it. Duplication is always the fallible `copy()` or `slice()`
method. It has no index syntax, iteration protocol, or hashing. A buffer can be
stored in collections, sent through channels, and moved into worker threads.

## The range rule

Every range must satisfy `0 <= start <= stop <= len`, and every index
`0 <= index < len`. There is no negative indexing and no clamping, unlike
[list slicing](../facilities/01-builtins/lists.md). A violation returns
`Err(BytesError.OutOfRange)` and changes nothing. A zero-length range is valid
anywhere from `0` to `len` inclusive. `overwrite` additionally requires
`offset + (stop - start) <= len` of the destination.

## Errors

```text
pub enum BytesError:
    OutOfRange
    InvalidUtf8
    Allocation(AllocError)
```

Every fallible operation in the module returns `Result[T, BytesError]`. The only
other result shape is `pop`, whose `Option[u8]` reports an empty buffer.
`Allocation` carries the storage `AllocError`: `OutOfMemory` when the allocator
refuses, `CapacityOverflow` when the requested total cannot be a size. A failed
operation leaves the buffer's length and contents unchanged and releases
anything it allocated.

## Views

A view of a buffer is an ordinary [reference](../language/05-ownership/02-borrowing.md)
plus explicit offsets: `&Bytes` is a shared view and `&mut Bytes` an exclusive
one. A function that works on part of a buffer takes the reference and a
`start`/`stop` pair and uses the checked operations above.

The existing loan rules therefore define what may happen while a view exists:

- while a `&Bytes` is live, the owner cannot be mutated, grown, cleared, moved,
  or exclusively borrowed;
- while a `&mut Bytes` is live, the owner cannot be read, borrowed, or moved;
- a buffer cannot be extended from itself, because the exclusive receiver
  conflicts with the shared source. Copy the range first.

These are compile-time `conflicting borrow` errors. Growth may relocate the
storage, but no address into it is ever exposed to a program, so relocation
cannot invalidate anything a program holds.

A first-class sub-range view value, one that carries its own bounds and can be
stored or returned, is not provided. It needs references stored in values,
which the language does not have yet; see the
[limits of returned references](../language/05-ownership/04-returned-references.md).

## Costs

Storage is one byte per element plus one fixed-size owner. `with_capacity`,
`slice`, `copy`, and the `from_` constructors reserve exactly what they need.
Growth through `push`, `extend`, `extend_range`, and `resize` is amortized and
may over-reserve; `reserve` requests an exact total.

| Allocates | Does not allocate |
|---|---|
| Constructors, `slice`, `copy`, `to_list`, `to_text` | `len`, `is_empty`, `capacity`, `get`, `find`, `==`, transport |
| `push`, `extend`, `extend_range`, `resize`, `reserve` beyond capacity | The same operations within capacity |
| | `set`, `fill`, `overwrite`, `truncate`, `pop`, `clear` |

`get`, `set`, `push` within capacity, `pop`, `truncate`, and `clear` take
constant time. Range operations take time linear in the range; `extend`,
`overwrite`, `slice`, `copy`, and `resize` use bulk copies. `fill` and `find`
are loops over single checked accesses.

Measured with a release compiler on x86-64 Linux, 8,388,608 elements, pinned to
one core:

| Program | `Bytes` | `list[u8]` |
|---|---|---|
| Build and copy once: peak resident memory | 18.9 MB | 526.8 MB |
| Build and copy once: wall time | 0.01 s (`resize` + `copy`) | 1.4 s (`append` loop + `copy`) |
| Build, then read every element: peak resident memory | 10.8 MB | 264.6 MB |
| Build, then read every element: wall time | 0.7 s (`resize` + `get` loop) | 1.6 s (`append` loop + index loop) |
| Append one element at a time | 0.6 s (`push` loop) | included above |

An empty program peaks at 2.4 MB. A single-element call costs roughly 70 to
85 ns here, so prefer the bulk operations; the saving is storage and copying.

## Transport

These methods move a range between a buffer and a descriptor without an
intermediate list. Each makes at most one native call, returns the bytes moved
as `Result[i64, IoError]`, and never changes the buffer's length or capacity.

| Method | Receiver | Contract |
|---|---|---|
| `read_file(file: &mut File, start, stop)` | exclusive | Read into `self[start:stop]`; zero for a nonempty range is end of input |
| `write_file(file: &mut File, start, stop)` | shared | Write a prefix of `self[start:stop]` |
| `read_stdin(start, stop)` | exclusive | Read standard input into `self[start:stop]` |
| `write_stdout(start, stop)`, `write_stderr(start, stop)` | shared | Write a prefix of the range to the process stream's buffer |
| `read_os_entropy(start, stop)` | exclusive | One `getrandom(2)` request into `self[start:stop]` |

A count below the range length is normal. A range outside the buffer is
`IoError.InvalidInput`; a closed file is `Closed`, and the open mode is
enforced with `NotReadable` and `NotWritable`. An interrupted call is
`IoError.System(4)`. File transfers are raw bytes with no newline translation.

Programs normally use these through [`std.io`](io.md), which adds retry, end
of input, seeking, and bounded buffering on top. `read_os_entropy` is the
single native primitive behind [`std.secrets`](secrets.md); import that module
for secret bytes and tokens.

The older [binary I/O builtins](../facilities/02-io/bytes-paths.md) and
[`std.path`](path.md) still take and return `list[u8]`. Convert explicitly with
`from_list` and `to_list`; each conversion allocates and copies.

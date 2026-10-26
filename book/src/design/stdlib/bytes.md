# std.bytes

`import std.bytes` provides `Bytes`, an owning contiguous buffer that stores one
byte per element, `BytesError`, and the `ByteSource` protocol. A `list[u8]` stores each element in a
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
| `std.bytes.from_bytes(value: bytes)` | `Result[Bytes, BytesError]` | Independent copy of a [byte literal](../language/03-types/bytes.md) |

`Bytes` has no public constructor and its storage field is private. Every
constructor allocates and can report `Allocation`.

A byte literal such as `b"\r\n"` has the separate, immutable prelude type
`bytes`, which lives in read-only data and never allocates. Operations that only
read another byte sequence accept a literal or a buffer through
[`ByteSource`](#byte-sources), without copying.

## Operations

`start`, `stop`, `index`, `offset`, `count`, and sizes are `i64`; elements are `u8`.

| Method | Receiver | Result | Contract |
|---|---|---|---|
| `len()` | shared | `i64` | Number of bytes |
| `is_empty()` | shared | `bool` | Whether the length is zero |
| `capacity()` | shared | `i64` | Bytes storable without reallocating |
| `get(index)` | shared | `Result[u8, BytesError]` | Read one byte |
| `find(value, start, stop)` | shared | `Result[Option[i64], BytesError]` | First index of `value` within the range |
| `find_in(set, start, stop)` | shared | `Result[Option[i64], BytesError]` | First index holding any byte of `set` |
| `find_not_in(set, start, stop)` | shared | `Result[Option[i64], BytesError]` | First index holding no byte of `set`: the end of a run |
| `find_bytes(needle, start, stop)` | shared | `Result[Option[i64], BytesError]` | First index where all of `needle` occurs inside the range |
| `count(value, start, stop)` | shared | `Result[i64, BytesError]` | Occurrences of `value` within the range |
| `matches(offset, source, start, stop)` | shared | `Result[bool, BytesError]` | Whether `source[start:stop]` occurs at `offset` |
| `matches_text(offset, text: &str)` | shared | `Result[bool, BytesError]` | Whether the UTF-8 bytes of `text` occur at `offset` |
| `ascii_run(start, stop)` | shared | `Result[i64, BytesError]` | Index after the run of bytes below 128 that begins at `start` |
| `utf8_run(start, stop)` | shared | `Result[i64, BytesError]` | Index after the longest valid UTF-8 that begins at `start` |
| `slice(start, stop)` | shared | `Result[Bytes, BytesError]` | Independent copy of a range, with exact capacity |
| `copy()` | shared | `Result[Bytes, BytesError]` | Independent copy of the whole buffer |
| `to_list()` | shared | `Result[list[u8], BytesError]` | Independent `list[u8]` for the transport builtins |
| `to_text()` | shared | `Result[str, BytesError]` | Strictly validated UTF-8 copy; otherwise `InvalidUtf8` |
| `to_text_range(start, stop)` | shared | `Result[str, BytesError]` | `to_text` of one range, validated and copied once with no intermediate `Bytes`; a range that splits a UTF-8 sequence is `InvalidUtf8` |
| `set(index, value)` | exclusive | `Result[(), BytesError]` | Replace one byte |
| `write_float(value: f64)` | exclusive | `Result[i64, BytesError]` | Write shortest scientific ASCII into existing bytes at zero; requires `len() >= 32`, returns bytes written, never allocates or changes length |
| `write_float_spec(value: f64, style: i64, precision: i64, offset: i64)` | exclusive | `Result[i64, BytesError]` | Write a Python format-spec spelling at `offset` for [`std.fmt`](fmt.md): style 1 fixed, 2 exponent, 3 general, 4 general keeping `.0`, 5 f64 and 6 f32 `print` spelling, plus 16 for `#`; a negative value starts with `-`. Returns bytes written, or the negated total length needed when shorter, writing nothing. Other styles, precision outside `0..=4096`, or an offset past the length are `OutOfRange`. Never allocates |
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

`source`, `set`, and `needle` parameters are `&S` for any `S: ByteSource`: a
`Bytes` or a `bytes` literal. `==` and `!=` compare contents. Printing a
buffer shows printable ASCII and writes every other byte as `\xhh`; it is a
diagnostic form, not a conversion.

`Bytes` is a resource: assignment and argument passing move it, and the builtin
`copy` rejects it. Duplication is always the fallible `copy()` or `slice()`
method. It has no hashing. A buffer can be stored in collections, sent through
channels, and moved into worker threads.

## Byte sources

```text
pub protocol ByteSource:
    def len(self) -> i64:
        pass
```

`ByteSource` names bytes that an operation reads in place. It has two
implementations: `Bytes`, and the built-in [`bytes`](../language/03-types/bytes.md),
whose one method is `len`. A parameter `source: &S` with `S: ByteSource`
accepts `&buffer`, `b"..."`, or a `bytes` value; each call is compiled for the
type passed, so neither form is copied or converted:

```plenty
import std.bytes
from std.bytes import ByteSource

def ends_line[S: ByteSource](line: &S) -> bool:
    count = line.len()
    count >= 2 and line[count - 2] == 13u8 and line[count - 1] == 10u8

def main() -> Result[(), std.bytes.BytesError | IoError]:
    mut head = std.bytes.from_text("Host: example")?
    head.extend(b"\r\n")?
    print(head.find_bytes(b": ", 0, head.len())?, ends_line(&head), ends_line(b"no"))?
    Ok(())
```
```output
Option[i64].Some(4) True False
```

The operations taking a `ByteSource` are `extend`, `extend_range`,
`overwrite`, `find_in`, `find_not_in`, `find_bytes`, and `matches` here, and
the stream writes in [`std.io`](io.md), [`std.net`](net.md), and
[`std.tls`](tls.md). Generic code is [checked for each concrete
type](../language/07-generics/protocols.md), so it can also index and iterate a
source, which both forms support. Other types may declare `len`, but the
operations above read a source's bytes natively and reject anything other than
`Bytes` and `bytes` when the call is compiled. The receiver of a `Bytes`
method is always a buffer: copy a literal with `from_bytes` to scan it with
`find` or `count`, or to change it.

## Index syntax and iteration

`buffer[i]` reads one byte as `u8` and `buffer[i] = value` replaces one. They
follow the [list index rules](../facilities/01-builtins/lists.md), not the
[method range rule](#the-range-rule): a negative index counts from the end, and
a missing index [traps](../language/08-errors/02-allocation-and-traps.md) with
exit status one and `index out of bounds: index I, length N` at the source
position. Use `get` and `set` when an out-of-range index is an expected outcome.

Writes need a `mut` binding, a mutable field, or `&mut Bytes`, and evaluate as
[indexed assignment](../language/04-expressions/assignment.md) does: the right
side, then the index, then the exclusive loan. `buffer[i] += 1u8` evaluates the
index once. Indexing works on named buffers, fields, references, and temporary
values; it never grows the buffer and allocates nothing.

`for b in buffer` consumes the buffer and yields each `u8`. `for b in &buffer`
keeps it and yields `&u8`; as for strings and ranges, each reference is to an
iteration-local copy, because single bytes are not addressable values.
`reversed(&buffer)` walks from the end. `for b in &mut buffer` is rejected;
assign through an index instead. The loop holds a shared loan, so its body
cannot change the buffer.

There is no slice syntax. A list slice copies, so `buffer[a:b]` would hide an
allocation; `slice(start, stop)` makes that copy explicit, and passing the buffer
with a range gives a view without one.

## Scanning

`find`, `find_in`, `find_not_in`, `find_bytes`, `count`, `matches`,
`matches_text`, `ascii_run`, and `utf8_run` each make one native call over
their range. A parser uses them to step from one significant byte to the next
without a `get` call per byte.

- A found index is absolute, never relative to `start`. `Nothing` means the
  range holds no match.
- A `set` is a `Bytes` whose elements are the members; order and repeats do not
  matter. An empty set matches no byte, so `find_in` returns `Nothing` and
  `find_not_in` returns `start` for a nonempty range. Build a set once and
  reuse it. The set, needle, or source may be the buffer being scanned.
- `find_bytes` reports a match only when the whole needle lies inside the
  range. An empty needle is found at `start`. It takes time linear in the
  range and needle lengths, even on repetitive input, and does not allocate.
- `matches` and `matches_text` require `0 <= offset <= len`. When fewer bytes
  remain after `offset` than the comparison needs, the result is `False`, not
  an error. An empty comparison is `True`.
- `ascii_run` and `utf8_run` return an index from `start` to `stop`; equal to
  `start` when the first byte does not qualify and to `stop` when every byte
  does. `utf8_run` accepts exactly what `to_text` accepts and excludes a
  sequence that `stop` cuts short.

## The range rule

Every range must satisfy `0 <= start <= stop <= len`, and every index passed to
a method `0 <= index < len`. Methods have no negative indexing and no clamping,
unlike [list slicing](../facilities/01-builtins/lists.md) and
[index syntax](#index-syntax-and-iteration). A violation returns
`Err(BytesError.OutOfRange)` and changes nothing. A zero-length range is valid
anywhere from `0` to `len` inclusive. `overwrite` additionally requires
`offset + (stop - start) <= len` of the destination. `matches` and
`matches_text` take an `offset` from `0` to `len` inclusive and never report a
short remainder as an error.

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
| Constructors, `slice`, `copy`, `to_list`, `to_text`, `to_text_range` | `len`, `is_empty`, `capacity`, `get`, index syntax, iteration, every scan, `==`, transport |
| `push`, `extend`, `extend_range`, `resize`, `reserve` beyond capacity | The same operations within capacity |
| | `set`, `fill`, `overwrite`, `truncate`, `pop`, `clear` |

`get`, `set`, index syntax, `push` within capacity, `pop`, `truncate`, and
`clear` take constant time. Range operations take time linear in the range;
`extend`, `overwrite`, `slice`, `copy`, `resize`, and `fill` are single native
bulk operations.

The scans run natively. `find` and `find_in` with a set of at most four bytes
examine eight bytes per step. `find_in` with a larger set and `find_not_in`
build a 256-entry table on each call, then examine one byte per step. `count`,
`ascii_run`, `utf8_run`, `matches`, and `matches_text` are linear in the bytes
examined. `find_bytes` searches for the needle's first byte and compares at
each candidate: linear for ordinary data, and at most the range length times
the needle length when the needle's first byte repeats throughout the range.

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

A scan call costs about the same as a `get` call, however long its range.
Finding the only comma at the end of a 67,108,864-byte buffer, same machine,
pinned to one core:

| Program | Wall time |
|---|---|
| `find(44u8, 0, len)` | 4 ms |
| `find_in` with a four-byte set | 18 ms |
| `find_not_in` with a 94-byte set | 15 ms |
| `count(44u8, 0, len)` | 17 ms |
| A `get` loop over the same range | 2.3 s |

Index syntax and iteration read the storage through a borrowed reference rather
than calling a `std.bytes` method. Reading every element of an 8,388,608-byte
buffer, same machine, pinned to one core:

| Loop | Wall time |
|---|---|
| `buffer.get(i)?` over `range(n)` | 0.56 s |
| `buffer[i]` over `range(n)` | 0.43 s |
| `for b in &buffer` | 0.15 s |

`fill` over 67,108,864 bytes takes about 5 ms.

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

Methods named `crypto_*`, and the functions `crypto_state` and
`crypto_keyed_state`, are the native back end of [`std.crypto`](crypto.md).
They operate only on states that module creates; import it instead. The
`compress_*` methods and `compress_state` are likewise the back end of
[`std.compress`](compress.md). `compress_state` rejects a capacity below 1
with `OutOfRange`, since a state with no output room could never make
progress.

The older [binary I/O builtins](../facilities/02-io/bytes-paths.md) and
[`std.path`](path.md) still take and return `list[u8]`. Convert explicitly with
`from_list` and `to_list`; each conversion allocates and copies.

## Acknowledgements

- `find_bytes` uses the [two-way](https://doi.org/10.1145/116825.116845) string-matching algorithm by Maxime Crochemore and Dominique Perrin (1991), as Rust's `core` string search does.

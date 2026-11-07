# std.bytes

`import std.bytes` provides `Bytes`, an owning contiguous buffer that stores one
byte per element, and `BytesError`. Use `Bytes` for
checked byte-range operations and byte-oriented I/O. A `list[u8]` also stores
one byte per element and provides ordinary list operations and element
references; its capacity is rounded to allocation units as described under
[list storage costs](../facilities/01-builtins/lists.md#storage-costs).
Bytes are never text: converting to `str` is an explicit, validated operation.

The module is embedded Plenty code over a private native storage owner. It is
available on the supported x86-64 Linux GNU target.

## Constructing

| Function | Result | Contract |
|---|---|---|
| `std.bytes.empty()` | `Result[Bytes, AllocError]` | Empty buffer with no reserved capacity |
| `std.bytes.with_capacity(capacity)` | `Result[Bytes, AllocError]` | Empty buffer with exactly `capacity` bytes reserved; negative is `CapacityOverflow` |
| `std.bytes.from_list(values: &list[u8])` | `Result[Bytes, AllocError]` | Independent copy of the list's bytes |
| `std.bytes.from_text(text: &str)` | `Result[Bytes, AllocError]` | Independent copy of the exact UTF-8 bytes |
| `std.bytes.from_bytes(source: &[u8])` | `Result[Bytes, AllocError]` | Independent copy of a [byte view](#byte-views): a literal, `Bytes`, `list[u8]`, or part of one |

`Bytes` has no public constructor and its storage field is private. Every
constructor allocates and can report `Allocation`.

A byte literal such as `b"\r\n"` has the separate, immutable prelude type
`bytes`, which lives in read-only data and never allocates. Operations that only
read another byte sequence take a [`&[u8]` view](#byte-views), so they accept a
literal, a buffer, a list, or part of any of them without copying.

`len(buffer)` calls its shared `len` method and returns the number of bytes,
just as `len` does for a byte literal or byte slice view. Membership operators
are unsupported on `Bytes`; use the explicit search methods below.

## Operations

`start`, `stop`, `index`, `offset`, `count`, and sizes are `i64`; elements are `u8`.

| Method | Receiver | Result | Contract |
|---|---|---|---|
| `len()` | shared | `i64` | Number of bytes |
| `is_empty()` | shared | `bool` | Whether the length is zero |
| `capacity()` | shared | `i64` | Bytes storable without reallocating |
| `get(index)` | shared | `u8` | Read one byte |
| `find(value, start, stop)` | shared | `Option[i64]` | First index of `value` within the range |
| `find_in(set, start, stop)` | shared | `Option[i64]` | First index holding any byte of `set` |
| `find_not_in(set, start, stop)` | shared | `Option[i64]` | First index holding no byte of `set`: the end of a run |
| `find_bytes(needle, start, stop)` | shared | `Option[i64]` | First index where all of `needle` occurs inside the range |
| `count(value, start, stop)` | shared | `i64` | Occurrences of `value` within the range |
| `matches(offset, source)` | shared | `bool` | Whether `source` occurs at `offset` |
| `matches_text(offset, text: &str)` | shared | `bool` | Whether the UTF-8 bytes of `text` occur at `offset` |
| `ascii_run(start, stop)` | shared | `i64` | Index after the run of bytes below 128 that begins at `start` |
| `utf8_run(start, stop)` | shared | `i64` | Index after the longest valid UTF-8 that begins at `start` |
| `slice(start, stop)` | shared | `Result[Bytes, AllocError]` | Independent copy of a range, with exact capacity |
| `copy()` | shared | `Result[Bytes, AllocError]` | Independent copy of the whole buffer |
| `to_list()` | shared | `Result[list[u8], AllocError]` | Independent `list[u8]` for the transport builtins |
| `to_text()` | shared | `Result[str, BytesError]` | Strictly validated UTF-8 copy; otherwise `InvalidUtf8` |
| `to_text_range(start, stop)` | shared | `Result[str, BytesError]` | `to_text` of one range, validated and copied once with no intermediate `Bytes`; a range that splits a UTF-8 sequence is `InvalidUtf8` |
| `set(index, value)` | exclusive | `()` | Replace one byte |
| `write_float(value: f64)` | exclusive | `i64` | Write shortest scientific ASCII into existing bytes at zero; requires `len() >= 32` and traps otherwise; returns bytes written, never allocates or changes length |
| `write_float_spec(value: f64, style: i64, precision: i64, offset: i64)` | exclusive | `i64` | Write a Python format-spec spelling at `offset` for [`std.fmt`](fmt.md): style 1 fixed, 2 exponent, 3 general, 4 general keeping `.0`, 5 f64 and 6 f32 `print` spelling, plus 16 for `#`; a negative value starts with `-`. Returns bytes written, or the negated total length needed when shorter, writing nothing. Other styles, precision outside `0..=4096`, or an offset past the length trap. Never allocates |
| `fill(start, stop, value)` | exclusive | `()` | Replace every byte of a range |
| `overwrite(offset, source)` | exclusive | `()` | Copy `source` over existing bytes at `offset`; never grows |
| `copy_within(start, stop, to)` | exclusive | `()` | Copy `self[start:stop]` to `to` in the same buffer, like `memmove`; the ranges may overlap; never grows |
| `push(value)` | exclusive | `Result[(), AllocError]` | Append one byte |
| `extend(source)` | exclusive | `Result[(), AllocError]` | Append `source`; pass `&other[a:b]` to append part of a buffer |
| `reserve(additional)` | exclusive | `Result[(), AllocError]` | Ensure room for `additional` more bytes |
| `resize(count, fill)` | exclusive | `Result[(), AllocError]` | Set the length, appending `fill` or dropping the tail |
| `truncate(count)` | exclusive | `()` | Shorten to `count`; a negative count or one longer than the length traps |
| `pop()` | exclusive | `Option[u8]` | Remove and return the last byte, or `Nothing` |
| `clear()` | exclusive | `()` | Set the length to zero, keeping capacity |

`source`, `set`, and `needle` parameters are `&[u8]` views. `==` and `!=`
compare contents. A buffer
[prints](#printing) as a byte literal.

`Bytes` is a resource: assignment and argument passing move it, and the builtin
`copy` rejects it. Duplication is always the fallible `copy()` or `slice()`
method. It has no hashing. A buffer can be stored in collections, sent through
channels, and moved into worker threads.

## Printing

`print`, `str.repr`, and f-string fields spell a `Bytes` buffer, a `bytes`
literal, and a `&[u8]` view the same way: as a byte literal that reads back as
the same bytes. Printable ASCII appears as itself, except that a backslash,
double quote, LF, CR, and TAB are `\\`, `\"`, `\n`, `\r`, and `\t`; every other
byte is `\xhh` with lowercase digits. This matches Python's `bytes` repr, except that the quotes are always
double. The private storage field never appears, also inside a class, list, or
`Option`.

```plenty
import std.bytes
import std.encoding
from std.bytes import BytesError
from std.encoding import EncodingError

def main() -> Result[(), AllocError | IoError | BytesError | EncodingError]:
    frame = std.bytes.from_bytes(b"GET\t\"/\"\r\n\x00\xff")?
    print(frame)?
    print(&frame[0:3], [std.bytes.from_text("ok")?]?)?
    print(std.encoding.hex_text(&frame)?)?
    Ok(())
```
```output
b"GET\t\"/\"\r\n\x00\xff"
b"GET" [b"ok"]
47455409222f220d0a00ff
```

The literal spelling suits data that is mostly ASCII, such as protocol text.
For binary data such as digests, keys, or packed records, write the hex form
with [`std.encoding.hex_text`](encoding.md), two lowercase digits per byte, or
format each byte explicitly.

## Byte views

An operation that only reads bytes takes `&[u8]`, a
[slice view](../language/04-expressions/slices.md#slice-views): a pointer and a
length. `&buffer`, `&buffer[a:b]`, `&list`, a `b"..."` literal, and a view held
by the caller all convert to it with no copy and no allocation. The range is
part of the view, so these operations take no `start` and `stop` and report no
range error: view bounds clamp, and a negative bound counts from the end.

```plenty
import std.bytes

def ends_line(line: &[u8]) -> bool:
    count = len(line)
    count >= 2 and line[-2] == 13u8 and line[-1] == 10u8

def main() -> Result[(), AllocError | std.bytes.BytesError | IoError]:
    mut head = std.bytes.from_text("Host: example")?
    head.extend(b"\r\n")?
    mut name = std.bytes.empty()?
    name.extend(&head[:4])?
    print(head.find_bytes(b": ", 0, head.len()), ends_line(&head), ends_line(b"no"), name)?
    Ok(())
```
```output
Option[i64].Some(4) True False b"Host"
```

The operations taking `&[u8]` are `extend`, `overwrite`, `find_in`,
`find_not_in`, `find_bytes`, and `matches` here, the stream writes in
[`std.io`](io.md), [`std.net`](net.md), and [`std.tls`](tls.md), and the
read-only inputs of [`std.crypto`](crypto.md). Each is one compiled body for
every byte owner; native operations read the view's bytes in place. A view has
`len()`, indexing, slicing, `==`, and iteration, but no methods. The receiver
of a `Bytes` method is always a buffer: copy a literal or view with `from_bytes` to
scan it with `find` or `count`, or to change it.

`std.bytes.ByteSource`, the protocol these operations took before, is
removed; naming it is a compile error that points to `&[u8]`.

## Index syntax and iteration

`buffer[i]` reads one byte as `u8` and `buffer[i] = value` replaces one. They
follow the [list index rules](../facilities/01-builtins/lists.md), not the
[method range rule](#the-range-rule): a negative index counts from the end, and
a missing index [traps](../language/08-errors/02-allocation-and-traps.md) with
exit status one and `index out of bounds: index I, length N` at the source
position. Use `get` and `set` when an out-of-range index is an expected outcome.

Inside a plain `unchecked:` suite under `--unchecked assume`, index syntax
requires `0 <= i < len(buffer)` and omits bounds checks and negative-index
normalization. Violations are undefined behavior. Default builds,
`unchecked trap:` and buffer methods retain their checks; see
[unchecked blocks](../language/03-types/numbers.md#unchecked-blocks).

Writes need a `mut` binding, a mutable field, or `&mut Bytes`, and evaluate as
[indexed assignment](../language/04-expressions/assignment.md) does: the right
side, then the index, then the exclusive loan. `buffer[i] += 1u8` evaluates the
index once. Indexing works on named buffers, fields, references, and temporary
values; it never grows the buffer and allocates nothing.

`for b in buffer` consumes the buffer and yields each `u8`. `for b in &buffer`
and `for b in &mut buffer` keep it and loop over a [slice view](#views) of the
whole buffer, as `&buffer[:]` does: each `&u8` or `&mut u8` points into the
buffer, so `*b += 1u8` changes it. `reversed(&buffer)` walks from the end. The
loop holds the view's loan, so its body cannot grow, shrink, or move the
buffer, and the body of a shared loop cannot write to it.

[Slice syntax](../language/04-expressions/slices.md) `buffer[a:b:c]?` copies the
selected bytes into a new `Bytes` with Python's clamping and returns
`Result[Bytes, AllocError]`; it also slices `bytes` literals. The `slice(start,
stop)` method keeps the [range rule](#the-range-rule): a range outside the
buffer traps instead of clamping. A [slice view](#views),
`&buffer[a:b]`, borrows the bytes without a copy.

## Scanning

`find`, `find_in`, `find_not_in`, `find_bytes`, `count`, `matches`,
`matches_text`, `ascii_run`, and `utf8_run` each make one native call over
their range. A parser uses them to step from one significant byte to the next
without a `get` call per byte.

- A found index is absolute, never relative to `start`. `Nothing` means the
  range holds no match.
- A `set` is a byte view whose elements are the members; order and repeats do not
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

Internal index arithmetic uses small explicit `unchecked:` regions only where
validated ranges or native scan bounds prove the result fits in `i64`.
Growable sizes remain native allocation results, and invalid ranges still trap
before native access. This does not make caller arithmetic
unchecked: index updates such as `buffer[i] += 1u8` follow the caller's ordinary
arithmetic contract.

Every range must satisfy `0 <= start <= stop <= len`, and every index passed to
a method `0 <= index < len`. Methods have no negative indexing and no clamping,
unlike [list slicing](../facilities/01-builtins/lists.md) and
[index syntax](#index-syntax-and-iteration). A violation is a caller bug: it
[traps](../language/08-errors/02-allocation-and-traps.md#trap-reports) with a
message naming the method and argument, such as
`Bytes.get: index is outside 0..len()`, and the report's note names the
calling line. Check a range that comes from input before passing it. A zero-length range is valid
anywhere from `0` to `len` inclusive. `overwrite` additionally requires
`offset + len(source) <= len` of the destination, and `copy_within` requires
`0 <= to` and `to + (stop - start) <= len`. `matches` and
`matches_text` take an `offset` from `0` to `len` inclusive and report a short
remainder as `False`. The same rule applies to the range arguments of
[`read_file` and `read_stdin`](#transport) and of
`to_text_range`.

Caller buffer ranges in stream reads, encoding offsets, UUID writes, crypto
output offsets, and CSV field positions also trap; see the [invalid-argument
convention](conventions.md#invalid-arguments).

## Errors

```text
pub enum BytesError:
    InvalidUtf8
    Allocation(AllocError)
```

The constructors and the methods that can fail only to allocate (`push`,
`extend`, `reserve`, `resize`, `copy`, `slice`, `to_list`) return
`AllocError`; a negative `reserve` or `resize` count is `CapacityOverflow`.
`to_text` and `to_text_range` return `BytesError` for `InvalidUtf8`. A range
outside the buffer traps. The only other result shape is
`pop`, whose `Option[u8]` reports an empty buffer.
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
- a buffer cannot be extended or overwritten from itself, because the
  exclusive receiver conflicts with the shared source. Use `copy_within` to
  move a range inside one buffer, or copy the range first.

These are compile-time `conflicting borrow` errors. Growth may relocate the
storage, but only a live loan can hold an address into it, and growth conflicts
with every live loan, so relocation cannot invalidate anything a program holds.

A [slice view](../language/04-expressions/slices.md#slice-views),
`&buffer[a:b]` or `&mut buffer[a:b]`, carries its own bounds: it is a `&[u8]`
or `&mut [u8]` of the selected bytes, and holds the same whole-buffer loan as
`&Bytes` or `&mut Bytes`. It supports `len`, index reads and writes, loops,
`iter` pipelines, narrower views, and `==`, and prints as a `bytes` literal,
`b"bc"`. A `&[u8]` parameter also accepts views of `list[u8]` and `bytes`
literals. It has no `Bytes` methods: a scan's receiver is the buffer and its
range, while the operations that read [another byte sequence](#byte-views) take
the view itself. A view cannot be stored or returned yet; that needs stored
references (backlog B11).

## Costs

Storage is one byte per element plus one fixed-size owner. `with_capacity`,
`slice`, `copy`, and the `from_` constructors reserve exactly what they need.
Growth through `push`, `extend`, and `resize` is amortized and
may over-reserve; `reserve` requests an exact total.

| Allocates | Does not allocate |
|---|---|
| Constructors, `slice`, `copy`, `to_list`, `to_text`, `to_text_range` | `len`, `is_empty`, `capacity`, `get`, index syntax, iteration, every scan, `==`, transport |
| `push`, `extend`, `resize`, `reserve` beyond capacity | The same operations within capacity |
| | `set`, `fill`, `overwrite`, `copy_within`, `truncate`, `pop`, `clear` |

`get`, `set`, index syntax, `push` within capacity, `pop`, `truncate`, and
`clear` take constant time. Range operations take time linear in the range;
`extend`, `overwrite`, `copy_within`, `slice`, `copy`, `resize`, and `fill` are
single native bulk operations.

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
| `buffer.get(i)` over `range(n)` | 0.56 s |
| `buffer[i]` over `range(n)` | 0.43 s |
| `for b in &buffer` | 0.15 s |

These timings predate issues #741 and #843. A `get` or `set` loop that cannot
resize or replace the buffer now reads its length and first-byte address once
and accesses each byte directly, with one comparison for an index the loop
does not prove in range; see the
[collections architecture](../toolchain/02-architecture/collections.md).

`fill` over 67,108,864 bytes takes about 5 ms.

## Transport

The read methods fill a range of a buffer from a descriptor, and the module
functions write a byte view to one, without an intermediate list. Each makes at
most one native call, returns the bytes moved as `Result[i64, IoError]`, and
never changes a buffer's length or capacity.

| Operation | Contract |
|---|---|
| `buffer.read_file(file: &mut File, start, stop)` | Read into `buffer[start:stop]`; zero for a nonempty range is end of input |
| `buffer.read_stdin(start, stop)` | Read standard input into `buffer[start:stop]` |
| `std.bytes.write_file(file: &mut File, source: &[u8])` | Write a prefix of `source` |

A count below the requested length is normal. A read range outside the buffer
traps under the [range rule](#the-range-rule); a write takes a view, which carries its own range.
A closed file is `Closed`, and the open mode is enforced with `NotReadable`
and `NotWritable`. An interrupted call is `IoError.System(4)`. File transfers
are raw bytes with no newline translation. `write_file` is a module function,
not a method, so a `b"..."` literal or a subview needs no owned buffer. For the
process streams, the prelude's
[`write_stdout_bytes` and `write_stderr_bytes`](../facilities/02-io/bytes-paths.md)
take the same `&[u8]` view and write all of it.

Programs normally use these through [`std.io`](io.md), which adds retry, end
of input, seeking, and bounded buffering on top.

`Bytes` has no native back-end methods. [`std.secrets`](secrets.md),
[`std.crypto`](crypto.md), [`std.compress`](compress.md),
[`std.db.sqlite`](sqlite.md), [`std.net`](net.md), [`std.ready`](ready.md) and
[`std.tls`](tls.md) call compiler intrinsics (`__crypto_*`, `__compress_*`,
`__tls_*`, `__sqlite_*`, `__net_*`, `__ready_*`, `__io_entropy_read`). They
read byte inputs as `&[u8]` views, passed as a pointer and a length with no
copy. Output buffers are `&mut Bytes` operands whose ranges Plenty checks
first; native handles are held in byte storage.
These intrinsics are reserved for the standard library: user code calling one
is rejected with `` `__crypto_update` is reserved for the standard library ``.
Import the module that owns them instead.

The older [binary I/O builtins](../facilities/02-io/bytes-paths.md) and
[`std.path`](path.md) still take and return `list[u8]`. Convert explicitly with
`from_list` and `to_list`; each conversion allocates and copies.


## Acknowledgements

- `find_bytes` uses the [two-way](https://doi.org/10.1145/116825.116845) string-matching algorithm by Maxime Crochemore and Dominique Perrin (1991), as Rust's `core` string search does.
- Word-at-a-time scans use the zero-byte test from Sean Eron Anderson's [Bit Twiddling Hacks](https://graphics.stanford.edu/~seander/bithacks.html#ZeroInWord), which credits a 1987 newsgroup post by Alan Mycroft.

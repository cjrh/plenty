# `std.text`

Bounded reusable text construction, joining, and scalar wrapping. Import
`std.text`; every allocating operation returns `Result[..., TextError]`.

| Operation | Behavior |
|---|---|
| `builder(max_bytes: i64)` | New empty `Builder`; a negative limit is `InvalidLimit` |
| `Builder.byte_len()`, `Builder.limit()` | Current UTF-8 byte count and configured maximum |
| `Builder.append(text: &str)` | Append a copy, or leave contents unchanged on failure |
| `Builder.clear()` | Empty the builder, retaining its allocation |
| `Builder.to_text()` | Independent UTF-8 string snapshot; builder remains reusable |
| `join(pieces: &list[str], separator: &str, max_bytes: i64)` | Separator between pieces, including empty pieces; empty list yields empty text |
| `wrap(text: &str, width: i64, max_bytes: i64)` | Hard wrap at `width` Unicode scalars, inserting LF; width must be positive |

`TextError` has `InvalidLimit`, `LimitExceeded`, `Allocation(AllocError)`, and
`Bytes(BytesError)` variants. Byte-buffer failures preserve their typed cause;
normalization uses the direct allocation variant. Limits apply to output bytes,
not characters. Appending checks the complete input size without allocating,
then encodes a temporary buffer and appends atomically. Invalid UTF-8 must be
handled at [byte decoding](bytes.md); `str` is always valid UTF-8. Embedded NUL
is preserved. Builders cannot expose or mutate incomplete UTF-8.

Wrapping preserves every input scalar and existing LF, including final LF.
It never inserts an extra LF before an existing LF. It does not collapse spaces,
find word boundaries, calculate terminal display width, or keep grapheme clusters
together: a combining mark can begin a new line. Width and string positions are
scalar counts; the output limit and `byte_len` are byte counts.

The builder owns a compact `Bytes` buffer. Methods borrow it; `append` and
`clear` require exclusive access, and ordinary loan checking rejects mutation
while shared access remains live. There is no global mutable state or thread
affinity. Appending takes O(input bytes) amortized time and a temporary copy of
that input; growth can temporarily retain old and replacement allocations.
Storage capacity grows geometrically and may exceed the logical byte limit;
the limit is not an exact resident-memory quota. `to_text` copies O(output bytes).
Joining and wrapping take O(input + output bytes) amortized time, plus output
storage. Wrapping currently allocates a small temporary byte buffer per scalar.

Algorithms are checked Plenty using `std.bytes`; no new OS backend is required.
Sources and storage primitives are bundled for offline compilation with a
relocated matching toolchain on x86_64 Linux GNU. See the
[runnable lesson](../../tutorial/08-resources/25-text-unicode.md).

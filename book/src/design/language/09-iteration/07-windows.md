# Shared windows and chunks

`source.windows(width)` and `source.chunks(width)` are intrinsic `for` and
comprehension sources over lists, `bytes`, `Bytes`, strings and their existing
shared views. They borrow the source once; they allocate no iterator, window
buffer or element copy. The result is not a storable iterator or a user-defined
iterator protocol.

```plenty
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4, 5]?
    for window in xs.windows(3):
        print(window)?
    for chunk in xs.chunks(2):
        print(chunk)?
    Ok(())
```

```output
[1, 2, 3]
[2, 3, 4]
[3, 4, 5]
[1, 2]
[3, 4]
[5]
```

`width` is an `i64`, evaluated once after the source. Zero and negative widths
produce no items. Windows overlap by all but one element and always have the
requested width; a shorter source yields nothing. Chunks do not overlap and
include a final short piece. Empty sources yield nothing. Huge positive widths
produce no windows and at most one chunk, without overflowing cursor arithmetic.

A list or byte source yields a shared slice view `&[T]` or `&[u8]`. Text yields
`&strview`, with widths and advances measured in Unicode scalars, consistently
with [substring views](../04-expressions/slices.md#substring-views). UTF-8 bytes
are never split. Use `text.as_bytes().windows(width)` for byte-width scanning.
Text loops keep byte-boundary cursors, so an overlapping advance decodes at
most two scalars after its initial width scan.

The source loan remains live through every loop backedge. Resizing, moving or
mutating the source while a window is live is rejected. Every yielded view is
shared, even when the source is writable. Shared overlapping windows may
coexist; forwarding or retaining one follows the existing
[borrowed-view origin rules](../05-ownership/04-returned-references.md), keeping its
owner borrowed until its last use. A view cannot outlive its owner. This feature
adds no exclusive windows, mutable chunks, heap-stored views, arbitrary user
iterators, or new borrowing-frame capabilities. Loops in closures and existing
externally borrowing generators follow their ordinary capture/frame rules;
borrowing an owned local across `yield` remains rejected.

Eager comprehensions reuse this loop plan. Pipeline sources such as
`iter(xs.windows(2))` are rejected: pipeline callbacks do not yet carry the
borrowed window item's origin. The source expression must have a known owner/view type.
Bind an inferred generic call result to a local first, or give explicit type
arguments, before using `.windows`/`.chunks`; this does not add general type
inference for chained iterator sources.

The cursor proves the whole selected range is in bounds, so window construction
has no range check, trap or caller-context update in the body. Indexing or
operations written inside the body retain their ordinary checks and error
contracts. In particular, copying a window still allocates; lending it to a
reader that accepts a view does not.

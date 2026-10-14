# std.fmt

`std.fmt` separates user-defined display from debug output and streams UTF-8 to
any static [`std.io.Writer`](io.md). It leaves builtin `print` unchanged.
Formatting policy is checked Plenty; the only new native primitive writes a
bounded floating-point spelling into existing byte storage. There is no runtime
reflection, format-string interpreter, or dynamic writer object.

## Writers and strings

`formatter(writer, scratch, max_bytes, max_depth)` consumes a writer and a
[`Bytes`](bytes.md) scratch buffer and returns `Result[Formatter[W], FormatError]`.
Scratch must already have **length at least 32**, not just capacity. The byte
budget must be nonnegative, and depth must be in `0..=64`; invalid arguments
return `InvalidSpec`. Construction consumes both arguments even on failure.
The formatter owns them; its public `writer` field lets the caller flush, close,
or inspect the underlying stream. Formatting never flushes or closes it.

`string_formatter(max_bytes, max_depth)` creates a growable in-memory cursor and
scratch. `finish(&output)` validates and copies its current bytes into a `str`;
the formatter remains usable. Both are fallible. `to_string(&value, max_bytes,
max_depth)` and `to_debug(...)` construct a formatter, invoke the corresponding
user contract below, and return a string. They return no partial string on error.
Use a formatter directly to inspect partial progress or reuse storage.

All formatting methods use an exclusive receiver and return
`Result[(), FormatError]`:

| Method | Form |
|---|---|
| `text(&str)` | Exact UTF-8, including embedded NUL |
| `debug_text(&str)` | Double quotes; escapes `"`, `\`, LF, CR, TAB; other C0/C1 controls use lowercase `\u{hex}`; other Unicode is preserved |
| `boolean(bool)` | `True` or `False` |
| `integer(i64, width)` | Signed decimal, including `i64.MIN` |
| `unsigned(u64, base, width)` | Base 2, 8, 10, or 16, lowercase digits, no prefix |
| `floating(f64, width)` | Shortest round-trip scientific decimal, e.g. `1.25e0`, `-0e0`, `inf`, `-inf`, `NaN` |

Smaller integer/float types can be explicitly converted. Width is a nonnegative
minimum **byte** count, padded on the left with ASCII spaces, including any sign.
It never truncates. Negative widths and unsupported bases fail before output.
These numeric spellings are ASCII, so byte and scalar widths coincide; no
grapheme or terminal-cell width is promised. Text has no implicit padding.
Floating formatting has no precision or locale controls; promoting `f32` to
`f64` spells the promoted value. Dynamic format specifications are unsupported.

## Static user contracts and aggregates

A public type opts in by providing either or both public methods:

```text
pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
    ...
pub def debug[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:
    ...
```

`std.fmt.display(&value, &mut out)` and `debug(...)` check the selected method at
each concrete generic instantiation. These are ordinary structural calls, not
named protocol bounds: this permits method-local `W` without extending the
language's monomorphic protocol requirements. Missing/private methods, wrong
signatures, moves, conflicting loans and worker effects get ordinary compiler
diagnostics. `to_string`/`to_debug` use these user methods; primitive scalars use
the explicit methods in the table instead of acquiring hidden methods.

`list_with(&values, &mut out, render)` writes `[a, b]` using a statically typed
callback `(&T, &mut Formatter[W]) -> Result[(), FormatError]` for each borrowed
element. `pair_with(&pair, &mut out, first, second)` similarly writes `(a, b)`
with separate callbacks for a heterogeneous pair. Callbacks choose display or
debug and may recursively use these helpers. Other tuple arities, maps, enums
and classes compose these primitives explicitly; there is no automatic derive.
See the [formatting lesson](../../tutorial/08-resources/formatting.md).

Display, debug, list and pair entrypoints each enter one depth level and restore
it on success **or error**. Depth zero permits scalars but rejects those
entrypoints. Custom recursive code must recurse through these entrypoints or
pair `enter()`/`leave()` itself; ordinary direct method recursion cannot be
policed. A custom renderer can still loop or deliberately emit unbounded work.

## Errors and partial output

`FormatError` has `InvalidSpec`, `LimitExceeded`, `RecursionLimit`,
`Bytes(BytesError)`, and `Output(StreamError)` variants. String/scratch storage
allocation failures are `Bytes(Allocation(...))`; sink allocation failures are
`Output(Allocation(...))`. `completed()` is the exact count accepted by the
writer since construction, including a successful prefix before any error.

The writer's positive short counts are retried, `Interrupted` is retried, zero
is `WriteZero`, and negative/oversized counts are `InvalidProgress`. An `Err`
must mean that call accepted no bytes, as required by `std.io`. There is no
rollback or implicit retry of an entire failed formatting operation. Retrying
it can duplicate its successful prefix. A partial OS write can split UTF-8;
`finish` then reports `Bytes(InvalidUtf8)` until valid text is restored.

The budget is checked before each emitted scalar, ASCII byte, or float spelling.
Limit errors never exceed it; previous output remains. Budgets count UTF-8 bytes,
escapes, delimiters and padding, not input characters. Direct writes through the
public `writer` field are outside the counter/budget contract. Callers must not
alter its position or contents during a formatting operation.

## Cost and availability

After construction, formatting scalars performs **no allocation** when the
writer does not allocate. The caller can preallocate a `Cursor` buffer to make
this true for in-memory streams. Scratch is reused without growth; string
output grows explicitly and `finish` copies the complete bytes. A custom
renderer/callback can allocate, and its ordinary effects are not suppressed.

Text is linear in input bytes plus output bytes, integer work is bounded by 64
digits plus padding, and f64 conversion has fixed bounded scratch work plus
padding. A padded float may be converted twice because padding reuses scratch.
List work is linear in element count plus callback work. At most one Unicode
scalar (four bytes), one ASCII byte, or one float spelling (at most 32 bytes) is
offered per write; wrap a syscall writer in `BufWriter` to batch these calls.
Memory is caller scratch plus writer state and bounded recursive call frames.

Formatters are affine because they own `Bytes`; copying is rejected. Loans and
worker transfers follow the underlying writer's normal ownership/effect rules.
The embedded module works offline with a relocated matching toolchain on the
supported x86-64 Linux GNU target and adds no external library dependency.

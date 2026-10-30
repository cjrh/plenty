# Python slice syntax

**Status: Implemented** (issue #579). Copying slices of lists, text, `Bytes`,
`bytes`, tuples and clists shipped as one change rather than the planned
sequence; the [slices reference](../../design/language/04-expressions/slices.md)
owns the contract. Shipped differently: tuple slices of affine components and
slices of temporary clists are rejected; Python-style `copy()` of such
components is not implemented. Element types containing `Shared` fields wait on
issue #582. Remaining: borrowed views `&xs[a:b]` (issue #581, B39/B11),
negative tuple indices (issue #583), and first-class `slice(...)` values. The
original text below was researched against local `main` at `f681a0d5` on
2026-10-30; the [implementation and documentation plan](slice-syntax/plan.md)
is a companion page.

## Problem

Slicing is among Python's most used features, and Plenty's Python-shaped syntax
makes people and coding agents reach for it first. B39 left it out because
`buffer[a:b]` would hide a copy. This proposal keeps Python's syntax and
results, and makes the copy visible the way list literals do: a slice returns
`Result[_, AllocError]` and needs `?`.

```text
def main() -> Result[(), AllocError | IoError]:
    xs = [0, 1, 2, 3, 4]?
    print(xs[1:3]?, xs[-2:]?, xs[::-1]?)?      # [1, 2] [3, 4] [4, 3, 2, 1, 0]
    print("Aé🙂Z"[1:3]?)?                      # é🙂
    Ok(())
```

## Decisions

These were settled with the user on the issue and in review of local PR #694:

1. A slice that produces a new value is fallible: `ys = xs[1:3]?`, typed
   `Result[_, AllocError]`.
2. Elements are duplicated with `copy()` semantics: deep and fallible in the
   same `AllocError`. `Shared[T]` elements are duplicated with `share()`.
   Element types that `copy()` rejects are a compile error naming the type.
3. Copying slices come first. Borrowed views will be spelled `&xs[1:3]`; the
   syntax reserves that form.
4. Out-of-range bounds clamp exactly as in Python, including negative indices
   and negative steps.
5. First-class `slice(...)` values are deferred.
6. Receivers: `list`, `str`, `Bytes`, `bytes`, tuples and `clist`.
7. Every slice returns a `Result`, tuples included, for consistency. The
   compiler may remove work it can prove unnecessary.
8. A `bytes` slice returns an owned `Bytes`, with no import needed.
9. `copy()` will duplicate nested `Shared` handles with `share()` (issue #582).
   Slices of elements that contain `Shared` fields depend on that change.
10. Tuple indexing will accept negative literals, as in Python: `t[-1]`
    (issue #583).
11. `str` slices always copy. `&` and `&mut` applied to a slice are rejected
    until borrowed views exist (issue #581).
12. `str.slice()` stays until slice syntax lands; whether to deprecate it is
    decided then, by its remaining use.
13. The step has a nonzero integer type, as `range` steps will (issue #584), so
    a zero step cannot be written or computed. See [the step](#the-step).

Plenty has no shared mutable aliasing, so a deep copy differs from Python's
shallow copy only where a Python program mutates an element through one list
and reads it through the other. Plenty can express that only through
`Shared[Mutex[T]]` elements, and slices `share()` those, which gives Python's
reference behavior.

## Syntax

A subscript whose brackets contain a top-level `:` is a slice:

```text
subscript = primary "[" expression "]"                       # index, unchanged
slice     = primary "[" [start] ":" [stop] [":" [step]] "]"
```

`xs[:]`, `xs[::]`, `xs[a:]`, `xs[:b]`, `xs[::c]` and `xs[a:b:c]` are all
slices. A comma inside slice brackets (`m[1:2, 3]`) is rejected; there are no
multi-dimensional subscripts. The parser builds a new `Slice` expression
rather than reusing `Index`, so later passes cannot confuse the two.

A slice is a postfix operation with the same precedence as indexing, calls,
member access and `?`. These parse as written:

| Source | Reading |
| --- | --- |
| `xs[1:3]?` | Copy, then propagate allocation failure |
| `xs[1:3]?[0]` | Index the copy |
| `len(xs[1:3]?)` | Length of the copy |
| `s[1:]?.upper()?` | Method on the copied text |
| `rows[0][1:]?` | Slice an element of a nested list |
| `&xs[1:3]` | Reserved for a borrowed view; rejected for now |

### No conflict with generic brackets

`NAME[...]` is ambiguous in today's parser: brackets after a name or member path
first parse as type arguments, and module resolution later picks the generic or
index reading. Issue #562 is a case where it picks wrongly: `TABLE[n]` on a
module constant is read as a generic call. A type-argument list can never
contain `:`, so `type_arguments` fails at the colon and the parser falls back
to the subscript path. No `Ambiguous` node is built, and `TABLE[n:]` works even
while `TABLE[n]` does not. Issue #562 remains a separate fix.

The only other expressions with a colon are anonymous functions,
`def(x: i64) -> i64: x + 1`, whose parser consumes its own colons, and dict
literals, which are inside braces. Neither can end a bound early.

## Bounds and normalization

Bounds are `i64` expressions, or references to `i64`, as indices are.
Unsuffixed literals infer `i64`; other integer types need an explicit cast such
as `i64(n)`. The step is a nonzero `i64`, described in [the step](#the-step);
an omitted step is `1`. Omitted bounds depend on the sign of the step, as in
Python. Positions count elements, Unicode scalars for `str`, bytes
for `Bytes` and `bytes`, and rows for `clist`.

Normalization follows CPython's `PySlice_Unpack` and `PySlice_AdjustIndices`.
This algorithm is normative:

```text
positions(length, start: Option[i64], stop: Option[i64], step: NonZero[i64]):
    # length >= 0. Arithmetic is exact: compute in i128, or first clamp a step
    # of i64.MIN to -i64.MAX as CPython does. Both give the same positions,
    # since a step of magnitude >= length selects at most one element.
    # step != 0 by its type; there is no zero case.
    if step > 0:
        low, high = 0, length                 # clamp targets
        first, last = 0, length               # omitted start, omitted stop
    else:
        low, high = -1, length - 1
        first, last = length - 1, -1          # -1 here means "before 0",
                                              # not the last element
    adjust(bound):
        if bound < 0:
            bound = bound + length
            if bound < 0:
                bound = low
        elif bound >= length:
            bound = high
        return bound
    s = adjust(start) if start is present else first
    e = adjust(stop) if stop is present else last
    if step > 0:
        count = (e - s - 1) // step + 1 if s < e else 0
    else:
        count = (s - e - 1) // (-step) + 1 if e < s else 0
    return s, s + step, s + 2 * step, ... (count positions)
```

The existing runtime helper `slice_bounds` in `plenty-runtime/src/aggregates.rs`
is the step-1 case of this algorithm; the implementation generalizes it.

These are the issue's examples with `a = [0, 1, 2, 3, 4]`. Each result matches
CPython 3, checked with `slice.indices` while writing this page.

| Expression | Meaning | Normalized `(s, e, step)` | Result |
| --- | --- | --- | --- |
| `a[1:3]` | `start` through `stop - 1` | `(1, 3, 1)` | `[1, 2]` |
| `a[1:]` | `start` through the end | `(1, 5, 1)` | `[1, 2, 3, 4]` |
| `a[:3]` | Beginning through `stop - 1` | `(0, 3, 1)` | `[0, 1, 2]` |
| `a[:]` | A copy of the whole sequence | `(0, 5, 1)` | `[0, 1, 2, 3, 4]` |
| `a[1:4:2]` | `start`, not past `stop`, by `step` | `(1, 4, 2)` | `[1, 3]` |
| `a[::2]` | Every second item | `(0, 5, 2)` | `[0, 2, 4]` |
| `a[-1]` | Last item: an index, not a slice | — | `4` |
| `a[-2:]` | Last two items | `(3, 5, 1)` | `[3, 4]` |
| `a[:-2]` | All except the last two | `(0, 3, 1)` | `[0, 1, 2]` |
| `a[::-1]` | All items, reversed | `(4, -1, -1)` | `[4, 3, 2, 1, 0]` |
| `a[1::-1]` | First two items, reversed | `(1, -1, -1)` | `[1, 0]` |
| `a[:-3:-1]` | Last two items, reversed | `(4, 2, -1)` | `[4, 3]` |
| `a[-3::-1]` | All except the last two, reversed | `(2, -1, -1)` | `[2, 1, 0]` |
| `a[4:1:-1]` | Down from 4, not reaching 1 | `(4, 1, -1)` | `[4, 3, 2]` |
| `a[10:20]` | Past the end: clamps | `(5, 5, 1)` | `[]` |
| `a[-100:100]` | Both sides clamp | `(0, 5, 1)` | `[0, 1, 2, 3, 4]` |
| `a[3:1]` | Empty, not reversed | `(3, 1, 1)` | `[]` |
| `a[::0]` | Zero step | — | compile error |

### Why clamp

Indexing traps on a missing position, and the `Bytes` methods return
`OutOfRange`. Slices clamp instead because that is Python's documented rule,
which users already rely on: `line[:80]` truncates, and `a[-3:]` takes at most
three. A trapping slice would turn correct Python habits into crashes. Clamping
is total, so clamping alone never causes an error. The existing `slice` methods
of lists and text already clamp.

### The step

A zero step selects no well-defined positions. Python raises `ValueError`, and
Plenty's `range` traps today. Neither suits Plenty's reliability goal: a zero
computed elsewhere, such as `n // k`, must not stop the program where a slice
uses it. So the step's type cannot hold zero. It is the nonzero integer type of
issue #584, called `NonZero[i64]` here; its exact construction API is decided
in that issue. `range` steps use the same type.

- An integer literal step converts at compile time: `xs[::2]`, `xs[::-1]` and
  `xs[1:10:3]` need no conversion.
- A literal `0`, or a constant that evaluates to zero, is a compile error.
- A step that is a run-time `i64` needs the explicit, fallible conversion that
  issue #584 defines, and the program handles the zero case where the value is
  created. A `NonZero[i64]` value can be passed directly.
- An omitted step is `1`.

The check happens once, where the value is created, not at each slice. A slice
has no zero-step trap and no zero-step error, and its type stays
`Result[_, AllocError]`. Slice syntax with a written step depends on issue
#584; slices without a step do not.

## Results by receiver

The receiver may be an owned value, a named place, a field, an element, or a
`&` or `&mut` reference. A slice only observes it: the source is unchanged, and
a mutable receiver is borrowed shared for the duration of the copy.

| Receiver | `x[a:b:c]` type | Selected values are duplicated by |
| --- | --- | --- |
| `list[T]` | `Result[list[T], AllocError]` | `copy()` semantics; `share()` for `Shared[T]` |
| `str` | `Result[str, AllocError]` | Copying the selected scalars' UTF-8 |
| `std.bytes.Bytes` | `Result[Bytes, AllocError]` | Copying bytes |
| `bytes` | `Result[Bytes, AllocError]` | Copying bytes out of read-only data |
| `tuple[...]` | `Result[tuple[...selected], AllocError]` | `copy()` semantics per component |
| `clist[T]` | `Result[clist[T], AllocError]` | Copying column values |

Every slice returns a `Result`, including empty slices, tuple slices of
copyable components such as `(1, 2, 3)[1:]`, and short strings that happen not
to allocate. One rule is easier to learn and to keep in mind than a list of
exceptions, and code does not change type when a bound changes. The compiler
may remove work it can prove unnecessary, such as the error path of a tuple
slice that cannot allocate.

Other receivers are rejected: `dict`, `set`, deques, ranges, generators,
`Shared[list[T]]` handles (slice `&*handle` once views exist, or index it), and
user classes. There is no `__getitem__`-style hook for slices yet.

### Lists

Each selected element is duplicated by its type's copy:

- Scalars, `@copy` classes and copyable aggregates are copied inline.
- `str` elements share their immutable storage, as `copy()` does.
- Affine elements that `copy()` accepts, such as nested lists, dictionaries,
  sets, tuples of those, and classes without `__del__`, are deep-copied. This is
  what makes `rows[1:]?` work for the common nested data.
- `Shared[T]` elements are duplicated with `share()`: a count increment, no
  allocation. The handles in both lists name the same values.
- Aggregates that contain `Shared` fields, such as `(str, Shared[Log])`, are
  copied with their handles shared once `copy()` does that (issue #582). Until
  then they are rejected, as `copy()` rejects them.
- Any other element type is a compile error naming the type. These include
  `Bytes` and other resources, generators, closures with captures, types with
  `__del__`, and aggregates that contain any of them.

For an owned temporary receiver, as in `load()[1:]?`, the implementation may
move the selected elements instead of copying them and drop the rest. Types
whose destruction is observable are rejected anyway, so the only difference is
fewer allocations. The existing `.slice(start, stop)` method keeps that
transfer as its contract and stays the way to take affine, non-copyable
elements out of a temporary.

Generic code is checked per specialization, so `xs[1:]?` in a function over
`&list[T]` reports a non-copyable `T` at the instantiation, naming the concrete
type.

### Strings

Positions are Unicode scalars, as for `len(text)` and `text[i]`; they are not
bytes and not grapheme clusters. `"Aé🙂Z"[::-1]` is `"Z🙂éA"`, and a combining
mark is reversed separately from its base.

A step-1 string slice copies. It does not share the source's storage:

- A `str` is one word pointing at a header followed by its UTF-8 bytes (see
  [value representations](../../design/toolchain/02-architecture/values.md#strings)).
  A substring that shares storage needs an offset and length, either a second
  word or a new header kind. Every string operation, hash and comparison would
  pay to check for it.
- A shared substring keeps its whole source alive. A ten-character field from a
  100 MB file would hold the file. Java changed `String.substring` to copy in
  JDK 7u6 for this reason, and Go substrings have the same retention cost. That
  conflicts with low memory use.
- The [string design](strings.md) chose one public `str` with no
  owning/view distinction. A view belongs to `&s[a:b]`, a separate type that
  borrows.

So the rule is the same for every `str` slice: it returns
`Result[str, AllocError]`. Two cases allocate nothing as an optimization, not
as a contract: results of at most seven UTF-8 bytes are stored inline (the
runtime's `try_build` already does this), and a whole-string step-1 slice may
retain the source's immutable storage instead of copying it.

### `Bytes` and `bytes`

`buffer[a:b:c]` on a `Bytes` returns a new `Bytes` with exact capacity, like
the `slice` method. The syntax clamps; the methods keep the
[range rule](../../design/stdlib/bytes.md#the-range-rule), under which an
out-of-range bound returns `BytesError.OutOfRange`. Index syntax already
follows list rules rather than method rules, so this is consistent within
`Bytes`. Because clamping removes `OutOfRange`, the error is `AllocError`, not
`BytesError`. Code whose error type is only `BytesError` keeps using
`slice(start, stop)` or widens its error type.

A `bytes` value refers to read-only data with its length in a header, so a
sub-range cannot be another `bytes` without a new representation, and new
read-only data cannot be created at run time. A `bytes` slice therefore
returns an owned `Bytes`: `b"GET /"[0:3]?` is the buffer `GET`. This is the
same copy as `std.bytes.from_bytes(value)?` followed by a slice, done in one
allocation. Using the result does not need `import std.bytes`; naming its type
does, as f-strings produce a `std.fmt.FormatError` without an import.

### Tuples

The result type depends on the bounds, so tuple slice bounds and step must be
integer literals, optionally negated (`t[-2:]`), or omitted. Any other bound is
rejected: `tuple slice bounds must be integer literals; the result type depends
on them`. The positions are computed at compile time with the arity as the
length:

| Source, `t: (i64, str, list[i64])` | Type |
| --- | --- |
| `t[1:]` | `Result[(str, list[i64]), AllocError]` |
| `t[:1]` | `Result[(i64,), AllocError]` |
| `t[::-1]` | `Result[(list[i64], str, i64), AllocError]` |
| `t[3:]` | `Result[(), AllocError]` |

An empty selection has the unit type `()`, which is also how Python spells the
empty tuple. Components are duplicated with `copy()` semantics, so the
`list[i64]` component is deep-copied and the source tuple is unchanged.
Tuple indexing originally accepted only nonnegative literals; issue #583 made
it accept negative literals too, so indices and slice bounds follow the same
rule.

### Columnar lists

`items[a:b:c]` on a `clist[T]` returns a new `clist[T]` holding the selected
rows. Fields are scalars, so no element copy can fail; only the new column
storage can. For a step other than one, each column is gathered with a stride.
Whole-clist `copy` is unsupported today, so `items[:]?` becomes the way to
duplicate one.

## Evaluation, borrowing and `&`

A slice evaluates the receiver place, then start, stop and step, left to right,
each once. It then normalizes the bounds and copies under a
shared loan of the receiver. Bounds may read the receiver, as in
`xs[len(xs) - 2:]?`, because the bounds are evaluated before the copy and only
shared loans are involved.

`&` and `&mut` directly applied to a slice are rejected, with or without `?`
and with or without parentheses:

```text
&xs[1:3]      # error: borrowed slice views are not implemented (B11/B39);
&xs[1:3]?     #        bind the copy first: `part = xs[1:3]?`, then `&part`
&mut xs[1:]   # error: likewise
```

`&xs[1:3]` is the planned view. Accepting `&xs[1:3]?` now as a borrow of a
temporary copy would leave two spellings one character apart, one copying and
one not, once views land. Rejecting both now keeps the view syntax free and the
cost of every accepted form plain. These rejections are phase-guard tests that
name issue #581, B11 and B39.

## Costs and allocation failure

For `k` selected elements, a slice does O(1) bound normalization and O(k)
copying, plus the cost of each element's copy:

| Receiver | Work | Allocations |
| --- | --- | --- |
| `list[T]`, copyable `T` | O(k) row copies or retains | List header, plus rows when `k > 0`; as `slice()` |
| `list[T]`, affine `T` | O(k) plus each element's deep copy | As above, plus each copy's allocations |
| `str`, ASCII | O(k) | One, or none when the result fits inline |
| `str`, other text | One scan over scalars to the farthest selected position; O(length) at worst, no auxiliary storage | One, or none when the result fits inline |
| `Bytes`, `bytes` | O(k) | As `Bytes.slice()` |
| Tuples | Each selected component's copy | Only those of the components' copies |
| `clist[T]` | O(k) per column | As `clist[T].with_capacity(k)` |

The count `k` is known before copying, so the result is reserved once with its
exact size; no slice grows its result. A `str` slice of non-ASCII text needs the
selected scalars' byte length before allocating, so it scans the selected
region twice.

If an allocation fails, the slice returns `Err(AllocError.OutOfMemory)`. Every
value already copied into the partial result is dropped, last copied first,
and the partial result is freed. The source is never changed, so retrying or
recovering needs no repair. `k` never exceeds the source length, so a slice
never reports `CapacityOverflow`.

Copying a slice only to iterate it wastes the copy. Lessons should teach
`for i in range(1, len(xs)):` or a [pipeline](../../design/language/09-iteration/06-pipelines.md)
over the source for that, until a view can be iterated with `for x in &xs[1:]`.

## Diagnostics

| Source | Message |
| --- | --- |
| `ys: list[i64] = xs[1:3]` | `slice returns Result[list[i64], AllocError]; use ? to propagate allocation failure`, matching the collection-literal message |
| `buffers[1:]?`, `buffers: list[Bytes]` | `slicing list[Bytes] copies each element, and Bytes cannot be copied`; hint: transfer from an owned temporary with `.slice(start, stop)`, or build a new list from explicit copies |
| `xs[::0]` | `slice step cannot be zero` |
| `xs[::k]`, `k: i64` | `slice step must be NonZero[i64]`; hint: the conversion from issue #584 |
| `xs[a:b] = ys`, `xs[a:b] += ys` | `slice assignment is not supported`; hint: assign elements by index, or build a new list |
| `&xs[1:3]`, `&mut xs[1:3]` | `borrowed slice views are not implemented yet`; hint: bind the copy first |
| `t[i:]`, `t: tuple` | `tuple slice bounds must be integer literals; the result type depends on them` |
| `m[1:2, 3]` | `a slice cannot have several dimensions` |
| `d[1:2]`, `d: dict` | `dict does not support slicing` |
| `xs[1.5:]` | The existing index-type diagnostic, naming `i64` |

The current "this resource cannot be copied" message does not name the type.
The slice diagnostic names the element type and the receiver type, since the
element is not written in the slice expression.

## Out of scope

| Feature | What it would need |
| --- | --- |
| Slice assignment `a[1:3] = values` | Python lets a step-1 assignment change the length, which can allocate. An assignment statement has no value for `?`, so this needs a fallible method such as `xs.splice(start, stop, values)?` or a statement-level failure rule. A stepped assignment requires equal lengths. The right side's elements would move in. |
| Deletion `del a[1:3]` | Plenty has no `del`. A `remove_range(start, stop)` method needs no allocation and can be added independently. |
| `slice(start, stop, step)` values | A prelude `slice` type with `Option[i64]` fields, and subscripts that dispatch on the operand's type; today they accept integers and dictionary keys. A `range` has required, typed bounds and is a sequence of positions; a `slice` is unresolved until applied to a length. `s.indices(length)` would return the normalized `range`. |
| Borrowed views `&xs[a:b]`, `&mut xs[a:b]` (issue #581) | A view type (address, length, stride) under the existing loan rules. Local-only views can follow the dictionary key-view precedent (issue #396); storing and returning them needs B11 stored references. `&mut xs[a:b]` would allow in-place sorts and fills of a sub-range. String views need byte offsets; `Bytes` sub-range views are B39's remaining item. |
| Ranges, deques, generators, user classes | Slicing a `range` could return a `range` without allocation; user classes need a hook design. Each needs its own use case. |

## Prior art

- **Python** copies for `list`, `str`, `bytes` and `tuple` slices, clamps, and
  allows negative steps. `memoryview` and NumPy slices are views. This proposal
  takes Python's results and adds the visible `?`.
- **Rust** spells views `&v[a..b]`: no copy, no negative positions, no step in
  the syntax (`.step_by` on iterators), and out-of-range bounds panic.
  `v.get(a..b)` returns an `Option`, and `.to_vec()` copies. Plenty's planned
  `&xs[a:b]` follows Rust's choice that a borrow is spelled with `&`.
- **Go** slices `s[a:b]` share the backing array, so `append` through one slice
  can overwrite elements visible through another. Out-of-range bounds panic, and
  there are no negative positions or steps. Its three-index form `s[a:b:c]` sets
  capacity, not a step. Plenty rejects shared mutable aliasing, so it cannot
  copy this design.
- **Mojo** keeps Python's `a[start:stop:step]` syntax through `__getitem__`
  with a `Slice` argument. Its `List` returns a new list, while `Span` and
  `StringSlice` borrow.
- **Nim** writes inclusive `a[1..3]` and counts from the end with `a[^2..^1]`.
  It copies into a new `seq` and raises on out-of-range bounds; `toOpenArray`
  borrows without copying.

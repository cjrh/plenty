# Building text

These methods observe their inputs once, receiver first, and return
`Result[str, AllocError]`. Inputs remain unchanged on success and failure; the
result outlives them. References are accepted. Length/layout overflow returns
`CapacityOverflow`; storage exhaustion returns `OutOfMemory`. Only the final
text needs storage, with no intermediate strings; [small outputs](text.md)
require no heap allocation. The one exception is `join` over a generator,
described [below](#joining).

| Operation | Behavior |
| --- | --- |
| `text.concat(other)` or `text + other` | Concatenate two strings |
| `separator.join(parts)` | Insert the separator between consecutive elements of an iterable of `str` |
| `text.repeat(count)` | Repeat exact contents; `count` is `i64`; zero or negative yields empty |
| `text[start:stop:step]` | [Slice](../../language/04-expressions/slices.md) by scalar position, with Python's clamping, omitted bounds, and steps |
| `text.slice(start, stop)` | Forward scalar slice with required `i64` bounds |
| `text.replace(old, new)` | Replace all non-overlapping literal matches, left to right |
| `text.removeprefix(prefix)` | Remove at most one exact prefix |
| `text.removesuffix(suffix)` | Remove at most one exact suffix |
| `text.strip()` | Remove Unicode White_Space at both ends |
| `text.lstrip()` / `text.rstrip()` | Remove White_Space at the left/right end |

Empty input to `repeat` produces empty output for any count.

## Joining

`join` accepts every iterable whose [`for` element](../../language/09-iteration/01-iterables.md)
is `str`, and joins exactly the pieces that loop would yield:

| Argument | Pieces | Allocations |
| --- | --- | --- |
| `list[str]` | Elements in order | One, for the result |
| `set[str]` | Elements, in no promised order | One, for the result |
| `dict[str, V]` | Keys in insertion order | One, for the result |
| `str` | Unicode scalars, so `"-".join("abc")` is `"a-b-c"` | One, for the result |
| `Generator[str]`, including a generator expression | Yielded values | Scratch growth plus one for the result |

An empty iterable produces empty text; a single piece adds no separator. Empty
pieces are retained. Other element types are rejected: write an f-string or
convert with `str.from` to join numbers.

Collections and strings are observed, with or without `&`. `join` measures
them before allocating, then copies once.

A generator is one-shot, so `join` consumes it as a `for` loop does: a named
generator moves (`sep.join(g)`), and a reference to one is rejected. Its total
size is unknown in advance. Pieces are appended to a scratch buffer whose
capacity starts at the first piece's size (at least 8 bytes) and at least
doubles each time it fills. The result is then copied into exact-size text and
the scratch is freed before `join` returns. Producing `n > 8` bytes takes at
most `ceil(log2(n / 8)) + 2` allocations and holds less than `3n` bytes at the
peak. For example, four 10-byte pieces with `", "` use scratch capacities of
10, 20, 40, and 80 bytes, then one 46-byte result. Each yielded piece is
released once it is copied. On failure the generator stops where it is; its
remaining state drops with it, and no partial text is returned. To bound the
cost, collect the pieces into a list first or use
[`std.text.join`](../../stdlib/text.md) with a byte limit.

Choose by the shape of the text:

- an [f-string](../../language/04-expressions/format-strings.md) for a fixed
  number of pieces of mixed types, such as `f"{host}:{port}"?`. It reports
  `std.fmt.FormatError`, not `AllocError`;
- `concat` (or `+`) for two strings in an `AllocError` function;
- `join` for any number of strings from one iterable.

`slice` includes start and excludes stop. Negative bounds count from the end;
bounds clamp to `[0, len(text)]`, without overflow for extreme `i64` values.
Reversed bounds yield empty text. ASCII bounds map directly to bytes; other
text requires scanning scalar boundaries. Slice syntax `text[a:b:c]?` follows the
same scalar positions and also accepts omitted bounds and a `NonZero[i64]` step;
`text[::-1]?` reverses the scalars.

`replace` observes receiver, old, and new in that order. Inserted text is not
searched again. An empty old string matches every scalar boundary, including
both ends; on empty input it inserts new once. Empty new text removes matches.
There is no count limit, regex interpretation, or normalization.

Prefix/suffix removal uses exact strings, including NUL, without repeated
removal. Empty or missing patterns preserve the contents. Trimming takes no
character-set argument, leaves interior text unchanged, and uses the same
White_Space classification as [isspace](text.md).

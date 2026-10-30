# Slices

A subscript whose brackets contain a top-level `:` is a slice. It copies the
selected elements into a new value and returns `Result[_, AllocError]`, so the
copy is visible as a `?` (issue #579):

```plenty
def main() -> Result[(), AllocError | IoError]:
    xs = [0, 1, 2, 3, 4, 5]?
    print(xs[1:3]?)?
    print(xs[-2:]?)?
    print(xs[::2]?)?
    print(xs[::-1]?)?
    print("Aé🙂Z"[1:3]?)?
    Ok(())
```

```output
[1, 2]
[4, 5]
[0, 2, 4]
[5, 4, 3, 2, 1, 0]
é🙂
```

## Syntax

```text
slice = primary "[" [start] ":" [stop] [":" [step]] "]"
```

`xs[:]`, `xs[::]`, `xs[a:]`, `xs[:b]`, `xs[::c]` and `xs[a:b:c]` are all
slices. A slice has the precedence of indexing and the other postfix forms:
`xs[1:3]?[0]` indexes the copy and `s[1:]?.upper()?` calls a method on it.
A comma inside slice brackets is rejected: there are no multi-dimensional
subscripts. The receiver, then each written bound, evaluate once from left to
right. The receiver is only read, so a named receiver stays usable.

## Bounds and steps

`start` and `stop` are `i64` expressions or references to `i64`; other integer
types need a cast such as `i64(n)`. Positions count elements, Unicode scalars
for `str`, bytes for `Bytes` and `bytes`, and rows for `clist`.

Out-of-range bounds clamp exactly as in Python. A negative bound counts from the
end, and positions outside the receiver select nothing rather than trapping.
Omitted bounds depend on the sign of the step: a positive step runs from the
start to the end, and a negative step runs from the last element back to the
first.

The step has type `NonZero[i64]` and defaults to `1`, so a zero step cannot be
written or computed. A nonzero integer literal converts implicitly, and a literal
`0` is a compile error. A computed step needs an explicit conversion, which
reports a zero step as `RangeError`:

```plenty
def main() -> Result[(), AllocError | IoError | RangeError]:
    xs = [0, 1, 2, 3, 4, 5]?
    step = NonZero.try_from(len(xs) // 2)?
    print(xs[::step]?)?
    print(xs[100:]?)?
    print(xs[-100:2]?)?
    Ok(())
```

```output
[0, 3]
[]
[0, 1]
```

## Receivers and results

| Receiver | Result | Elements |
| --- | --- | --- |
| `list[T]` | `Result[list[T], AllocError]` | Duplicated with `copy()`; `Shared[T]` handles with `share()` |
| `str` | `Result[str, AllocError]` | Unicode scalars |
| `std.bytes.Bytes` | `Result[std.bytes.Bytes, AllocError]` | Bytes |
| `bytes` | `Result[std.bytes.Bytes, AllocError]` | Bytes; no import needed |
| `clist[T]` | `Result[clist[T], AllocError]` | Rows copied column by column |
| Tuple | `Result[(...), AllocError]` | Components selected at compile time |

A list slice of a nested list copies each selected inner list, so later changes
to the source do not reach the slice. A step-1 slice of a list whose elements
own no storage, such as `list[i64]`, is one block copy. Every slice reserves its
result storage once, before copying; allocation failure returns
`Err(AllocError.OutOfMemory)` and leaves the receiver unchanged.

A list element type that `copy()` rejects, such as `std.bytes.Bytes`, is a
compile error. Transfer such elements out of an owned temporary with the
[`slice()` method](../../facilities/01-builtins/lists.md#slices) instead. Element
types that contain `Shared` fields are also rejected until `copy()` shares
nested handles (issue #582).

A `bytes` literal slice gives an owned `std.bytes.Bytes`. Slice syntax makes the
`Bytes` type available without an `import std.bytes` line; naming the type in a
signature still needs the import. The compiler loads `std.bytes` for this only
when some module of the program could produce a `bytes` value: one with a
`b"..."` literal, `include_bytes`, or the word `bytes`. A program that slices
only lists or text does not load it.

Tuple slice bounds and step must be integer literals, because the result type
depends on them: `(1, "two", 3.0)[1:]?` has type `Result[(str, f64), AllocError]`.
A tuple whose selected components are affine is rejected for now; unpack it and
copy the components instead.

A clist receiver must be a named place, and the slice is subject to the same
borrow checks as other [clist](../../facilities/01-builtins/clists.md) reads.

## Limits

| Source | Diagnostic |
| --- | --- |
| `ys: list[i64] = xs[1:]` | ``slice returns Result[list[i64], AllocError]; use `?` to propagate allocation failure`` |
| `xs[::0]` | `slice step cannot be zero` |
| `xs[::k]` with `k: i64` | ``slice step must be `NonZero[i64]`, not `i64` `` |
| `&xs[1:3]`, `&mut xs[1:3]` | `borrowed slice views are not implemented yet; bind the copy first` |
| `xs[1:3] = ys` | `slice assignment is not supported; assign elements by index, or build a new list` |
| `m[1:2, 0]` | `a slice cannot have several dimensions` |
| `t[n:]` on a tuple | `tuple slice bounds must be integer literals; the result type depends on them` |
| `d[1:]` on a dict | `dict does not support slicing` |

```plenty-error
def main() -> Result[(), AllocError]:
    xs = [1, 2, 3]?
    view = &xs[1:3]
    Ok(())
```

```error
borrowed slice views are not implemented yet
```

Borrowed views spelled `&xs[a:b]` are planned (issue #581, backlog B39); they
need stored references (B11). First-class `slice(...)` values are not
implemented. The [slice syntax proposal](../../../proposals/completed/slice-syntax.md)
records the design and alternatives.

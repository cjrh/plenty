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
[`slice()` method](../../facilities/01-builtins/lists.md#slices) instead. A
`Shared` handle in an element, as in `list[(str, Shared[T])]`, gains another
owner, as [`copy()`](../05-ownership/01-bindings-moves-and-copies.md) does.

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

## Slice views

`&xs[a:b]` borrows the selected elements in place, and `&mut xs[a:b]` borrows
them exclusively (issue #581). A view copies nothing and allocates nothing, so it
needs no `?`. Use one to pass part of a list or buffer to a function:

```plenty
def total(values: &[i64]) -> i64:
    mut sum = 0
    for x in values:
        sum += *x
    sum

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4, 5]?
    print(total(&xs[1:4]))?
    tail = &mut xs[-2:]
    tail[0] = 40
    for x in &mut xs[:2]:
        *x *= 10
    print(xs)?
    Ok(())
```

```output
9
[10, 20, 3, 40, 5]
```

The bounds clamp as for a copying slice, so a view is empty rather than out of
range. A view selects adjacent elements, so it takes no step; copy every k-th
element with `xs[a:b:k]?`.

| Viewed value | Shared view | Exclusive view |
| --- | --- | --- |
| `list[T]` | `&[T]` | `&mut [T]` |
| `std.bytes.Bytes` | `&[u8]` | `&mut [u8]` |
| `bytes` | `&[u8]` | rejected: a literal is read-only |
| A view | A narrower view | A narrower view of an exclusive view |
| `Shared[list[T]]` | `&[T]` of the content | rejected: the content is shared |

`&[T]` and `&mut [T]` are types: parameters and local annotations may name
them. A list of `u8`, a `Bytes` buffer, and a `bytes` literal all give `&[u8]`,
so one `&[u8]` parameter reads any of them. The viewed value must be a named
place, such as `xs`, `self.items`, or `rows[0]`.

| Operation | Meaning |
| --- | --- |
| `len(v)` | Number of elements in the view |
| `v[i]` | Element `i` of the view, with list index rules: a negative index counts from the end, and an index outside the view traps |
| `v[i] = x` | Replace element `i`; needs an exclusive view in a `mut` binding |
| `&v[i]`, `&mut v[i]` | Reference to element `i` |
| `&v[a:b]`, `&mut v[a:b]` | Narrower view; bounds count from the start of `v` |
| `for x in v` | `&T` per element, or `&mut T` for an exclusive view; `reversed(v)` and `enumerate(v)` also work |
| `v == w`, `v != w` | Equal lengths and equal elements; both sides are views of one element type |

Other list operations, such as methods, `in`, formatting, `list(v)`, and
copying slices of a view, are rejected. Copy from the owner, `xs[a:b]?`, for an
owned value.

### Views borrow their owner

A view holds a loan of its whole owner: a shared view a shared loan, and an
exclusive view an exclusive one, under the [borrowing rules](../05-ownership/02-borrowing.md).
While a view is live, its owner cannot grow, shrink, move, or be reassigned.
Two views of one list are never disjoint, so `&mut xs[:2]` and `&xs[2:]`
conflict while both are live.

A `&[T]` or `&mut [T]` parameter takes a view, `&xs[a:b]` or a view binding.
A list is not converted to a view implicitly; pass the whole list as `&xs[:]`.
A function with a view parameter may return a reference into it: the first
reference or view parameter is the
[origin](../05-ownership/04-returned-references.md) of a returned reference.
Views may be passed to [scoped workers](../../facilities/03-concurrency/index.md).

Views are local until stored references land (backlog B11). A view cannot be
returned, stored in a class field, tuple, collection, or enum payload, or
captured by an anonymous function. Return or store the bounds, or an owned copy.

`str` has no slice views. Its positions count Unicode scalars, so finding a
bound scans the text, while a list or byte view finds its bounds in constant
time. Copy part of a string with `s[a:b]?`.

## Limits

| Source | Diagnostic |
| --- | --- |
| `ys: list[i64] = xs[1:]` | ``slice returns Result[list[i64], AllocError]; use `?` to propagate allocation failure`` |
| `xs[::0]` | `slice step cannot be zero` |
| `xs[::k]` with `k: i64` | ``slice step must be `NonZero[i64]`, not `i64` `` |
| `&xs[::2]` | `a slice view selects adjacent elements, so it takes no step` |
| `&s[1:3]` on a `str` | `str has no slice views` |
| `def f(xs: &list[T]) -> &[T]` | `a function cannot return a slice view yet (backlog B11)` |
| `total(&xs)` for `total(v: &[i64])` | ``this parameter takes a slice view, `&xs[a:b]`; pass a whole list or buffer as `&xs[:]` `` |
| `print(v)` for a view `v` | `slice views cannot be formatted` |
| `x in v` for a view `v` | ``slice views do not support `in` yet`` |
| `xs[1:3] = ys` | `slice assignment is not supported; assign elements by index, or build a new list` |
| `m[1:2, 0]` | `a slice cannot have several dimensions` |
| `t[n:]` on a tuple | `tuple slice bounds must be integer literals; the result type depends on them` |
| `d[1:]` on a dict | `dict does not support slicing` |

```plenty-error
class Window:
    rows: &[i64]

def main() -> ():
    pass
```

```error
slice views cannot be stored in class fields yet (backlog B11)
```

First-class `slice(...)` values are not implemented. The [slice syntax proposal](../../../proposals/completed/slice-syntax.md)
records the design and alternatives.

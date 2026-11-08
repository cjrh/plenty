# Slices

A subscript whose brackets contain a top-level `:` is a slice. It copies the
selected elements into a new value and returns `Result[_, AllocError]`, so the
copy is visible as a `?` (issue #579). A tuple slice allocates nothing and
returns the tuple itself (see [tuple slices](#tuple-slices)):

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
| Tuple | The selected tuple, or `()`; no `Result` | Components selected at compile time |

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

### Tuple slices

Tuple slice bounds and step must be integer literals, because the result type
depends on them: `(1, "two", 3.0)[1:]` has type `(str, f64)`. A slice that
selects no component is `()`. A tuple slice copies components inline and
allocates nothing, so it returns no `Result` and takes no `?` (issue #625):

```plenty
def main() -> Result[(), IoError]:
    t = (1, "two", 3.0)
    tail: (str, f64) = t[1:]
    print(tail, t[::-1])?
    Ok(())
```

```output
("two", 3.0) (3.0, "two", 1)
```

A tuple whose selected components are affine is rejected for now; unpack it and
copy the components instead.

A clist receiver must be a named place, and the slice is subject to the same
borrow checks as other [clist](../../facilities/01-builtins/clists.md) reads.

## Slice views

`&xs[a:b]` borrows the selected elements in place, and `&mut xs[a:b]` borrows
them exclusively (issue #581). A view copies nothing and allocates nothing, so it
needs no `?`. Use one to pass part of a list or buffer to a function:

```plenty
def total(values: &[i64]) -> Result[i64, OverflowError[i64]]:
    mut sum = 0
    for x in values:
        sum += *x
    Ok(sum)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut xs = [1, 2, 3, 4, 5]?
    print(total(&xs[1:4])?)?
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
place, such as `xs`, `self.items`, or `rows[0]`, or a `bytes` or `str`
literal, as in `&b"GET /"[:3]`. A shared view argument may also view a call
result, as in `w.write(&encode(x)?[2:])`: the result lives until the end of the
statement. Bind a call result to a name before binding a view of it to a name,
and before passing an exclusive view of it.

| Operation | Meaning |
| --- | --- |
| `len(v)` | Number of elements in the view |
| `v[i]` | Element `i` of the view, with list index rules: a negative index counts from the end, and an index outside the view traps |
| `v[i] = x` | Replace element `i`; needs an exclusive view in a `mut` binding |
| `v.swap(i, j)` | Exchange elements `i` and `j` in place, with [`list.swap`](../../facilities/01-builtins/lists.md#reordering-and-extending)'s contract; needs an exclusive view |
| `&v[i]`, `&mut v[i]` | Reference to element `i` |
| `&v[a:b]`, `&mut v[a:b]` | Narrower view; bounds count from the start of `v` |
| `for x in v` | `&T` per element, or `&mut T` for an exclusive view; `reversed(v)` and `enumerate(v)` also work |
| `iter(v)` | A [pipeline](../09-iteration/06-pipelines.md) over the elements, with the items of `for x in v` |
| `v == w`, `v != w` | Equal lengths and equal elements; both sides are views of one element type |
| `print(v)`, `str.repr(v)`, `f"{v}"` | The elements as a list, `[2, 3]`; a `&[u8]` as a `bytes` literal, `b"bc"` |

In `--unchecked assume` builds, view indexing inside a plain
[`unchecked:` suite](../03-types/numbers.md#unchecked-blocks) assumes a
nonnegative index below the view's own length, with no bounds check or
negative-index normalization. The view's construction and `swap` method keep
their normal contracts.

A view's length never changes, so `v[i]` in `for i in range(len(v))` or
`for i, x in enumerate(v)` compiles without a bounds check, as described for
[lists](../../facilities/01-builtins/lists.md). An in-place reversal,
`while lo < hi:` starting with `v.swap(lo, hi)` or `v[hi]`, checks its
first pass's indices once, before the loop, as described there.

```plenty
def even(x: &i64) -> bool:
    two: NonZero[i64] = 2
    *x % two == 0

def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4, 5]?
    middle = &xs[1:4]
    print(middle, iter(middle).filter(even).count())?
    data = b"abc"
    print(&data[1:])?
    Ok(())
```

```output
[2, 3, 4] 2
b"bc"
```

A view prints as a reference does, through to its elements; a `Bytes` buffer
itself keeps its own spelling. Other list operations, such as methods other
than `swap`, `in`, `list(v)`, and copying slices of a view, are rejected. Copy
from the owner, `xs[a:b]?`, for an owned value.

`v.swap(i, j)` counts both indices within the view, so a negative index counts
from the view's end, and an index outside the view traps before either element
moves. Both indices are evaluated before the view is borrowed, so they may read
it, as `len(v) - 1` does. Equal indices leave the view unchanged. It accepts
every element type and allocates, copies, retains, and drops nothing. For
numbers and `bool` it compiles to the inline bounds checks, loads, and stores
of `list.swap`; other element types make one runtime call, which repairs inline
payload addresses. A shared view is read-only and has no `swap`:

```plenty
def reverse(v: &mut [i64]) -> Result[(), OverflowError[i64]]:
    mut lo = 0
    mut hi = len(v) - 1
    while lo < hi:
        v.swap(lo, hi)
        lo += 1
        hi -= 1
    Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut xs = [1, 2, 3, 4, 5]?
    reverse(&mut xs[1:])?
    print(xs)?
    Ok(())
```

```output
[1, 5, 4, 3, 2]
```

### Views borrow their owner

A view holds a loan of its run of elements in its owner: a shared view a
shared loan, and an exclusive view an exclusive one, under the
[borrowing rules](../05-ownership/02-borrowing.md). While a view is live, its
owner cannot grow, shrink, move, or be reassigned, and `len(xs)` and `&xs`
conflict with an exclusive view. Two views are disjoint when one stops where
the other starts, at one literal or non-`mut` `i64` name, so `&mut xs[:m]` and
`&mut xs[m:]` can both be live. Overlapping or unprovable ranges, such as
`&mut xs[:3]` with `&xs[2:]`, conflict.
[Disjoint slice views](../05-ownership/02-borrowing.md#disjoint-slice-views)
states the full rule and why it is sound.

Where a view type is expected, a borrow of its owner converts to the whole
view, as `[:]` would: `&xs` or `&mut xs` for a list, `&buffer` or a `b"..."`
literal for `&[u8]`, and `&handle` for a `Shared` list. A reference binding to
an owner, such as an `xs: &list[i64]` parameter, converts too. The conversion
copies nothing and allocates nothing. Expected view types are parameters,
including a generic `&[T]`, and annotated locals:

```plenty
def total(values: &[i64]) -> Result[i64, OverflowError[i64]]:
    mut sum = 0
    for x in values:
        sum += *x
    Ok(sum)

def width(data: &[u8]) -> i64:
    len(data)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    xs = [1, 2, 3]?
    whole: &[i64] = &xs
    print(total(&xs)?, total(&xs[1:])?, total(whole)?, width(b"abc"))?
    Ok(())
```

```output
6 5 6 3
```

`&xs[:]` keeps working. An owned list, `total(xs)`, is not a view and is
rejected. A `&T` type parameter takes a reference to a whole value, not a
view. Standard-library operations that read bytes take
[`&[u8]`](../../stdlib/bytes.md#byte-views), so `buffer.extend(&other[2:])` and
`writer.write(b"ok")` pass a view.
A function with a view parameter may return a reference into it: the first
reference or view parameter is the
[origin](../05-ownership/04-returned-references.md) of a returned reference.
Views may be passed to [scoped workers](../../facilities/03-concurrency/index.md).

Views are local until stored references land (backlog B11). A view cannot be
returned, stored in a class field, tuple, collection, or enum payload, or
captured by an anonymous function. Return or store the bounds, or an owned copy.

### Substring views

`&s[a:b]` on a `str` borrows the selected text in place as a `&substr`
(issue #595). Like other views it copies nothing, allocates nothing, and takes
no `?`. Its bounds are those of the copying slice `s[a:b]?`: they count
Unicode scalars, a negative bound counts from the end, and bounds outside the
text clamp. A view therefore always starts and ends on a scalar boundary, and
`s[a:b]? == &s[a:b]` for any `a` and `b`.

```plenty
def vowels(text: &substr) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    for c in text:
        if c in "aeiouéö":
            n += 1
    Ok(n)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    s = "héllo, wörld"
    word = &s[7:]
    print(word, len(word), word.byte_len(), vowels(&s[:5])?)?
    print(word == "wörld", &s[:1] < word, "ör" in word, word.find("l"))?
    for c in reversed(&word[1:3]):
        print(c)?
    owned = word[:2]?
    print(owned)?
    Ok(())
```

```output
wörld 5 6 2
True True True Option[i64].Some(3)
r
ö
wö
```

`&substr` is a separate type from `&str`. A `&str` refers to a whole `str`,
whose [representation](../../toolchain/02-architecture/values.md#strings)
stores its byte length and scalar count ahead of the text, and every `str`
operation reads them. A run inside a string has no such prefix, so a view
holds the run's address and byte length instead. `substr` is only written as
`&substr`; there is no owned `substr` and no `&mut substr`, since a `str` is
immutable. A `&substr` parameter also takes a whole string: `f(s)` implicitly
borrows it, and `f(&s)`, a `&str` binding, or a `str` literal work too. Each
converts as `&s[:]` would, without copying, retaining, or allocating. Owned
temporary arguments are evaluated once and stay alive through the containing
full expression, including later arguments and error propagation. An annotated
local still needs a borrow, such as `whole: &substr = &s`, or a literal;
implicit borrowing applies to calls.
The reverse does not hold: a `&str` parameter cannot take a `&substr`
without a copy. Declare the parameter `&substr` to accept both a view and a
whole string, or pass an owned copy, `f(s[a:b]?)`, which the call lends as a
temporary.

| Operation | Meaning |
| --- | --- |
| `len(v)` | Number of scalars; scans the view's text, as positions do |
| `v.byte_len()` | Number of UTF-8 bytes; constant time |
| `v.as_bytes()` | Shared `&[u8]` over the selected UTF-8 bytes, without copying, allocating or scanning |
| `v[i]` | Scalar `i` as a one-scalar `str`; a negative index counts from the end, and an index outside the view traps |
| `&v[a:b]` | Narrower view; bounds count scalars from the start of `v` |
| `v[a:b]?` | An owned copy of part of the view, `Result[str, AllocError]` |
| `for c in v` | Each scalar as a one-scalar `str`; `reversed(v)`, `enumerate(v)`, and `iter(v)` also work |
| `v == w`, `v != w`, `<`, `<=`, `>`, `>=` | Compare by Unicode scalar value, as `str` does; either side may be a `str` or a `&substr` |
| `needle in v`, `needle not in v` | Substring membership; the needle is a `str` or a `&substr` |
| `v.startswith(x)`, `v.endswith(x)` | Exact prefix or suffix; `x` is a `str` or a `&substr` |
| `v.find(x)` | First matching scalar position within the view, or `Nothing` |
| `print(v)`, `f"{v}"` | The text itself; `str.repr(v)` and `f"{v!r}"` quote it as a `str` |

Each scalar a loop or index yields fits in a string's inline form, so these
operations allocate nothing. Finding a bound, `len(v)`, `v[i]`, and `v.find(x)`
scan the text before the position, as they do on a `str`. `str` methods other
than those listed are not available on a view; copy it with `v[:]?` to use one.
A `str` method does not take a `&substr` argument, and `v in s` for a `str`
`s` is written `v in &s[:]`.

The byte view returned by `as_bytes()` keeps the text view's original loan,
including inline bytes stored in a string slot. It can be bound locally, passed
to any `&[u8]` reader, narrowed by byte positions, indexed, and iterated as an
ordinary byte view. It grants no mutable access to string storage. General
returned/stored views remain unsupported; `as_bytes()` is a checked builtin
borrow operation, not permission for arbitrary functions to return views.

A substring view borrows its string as other views borrow their owner: while
it is live, the string cannot be reassigned or moved, and the view stays local
until stored references land (backlog B11).

## Limits

| Source | Diagnostic |
| --- | --- |
| `ys: list[i64] = xs[1:]` | ``slice returns Result[list[i64], AllocError]; use `?` to propagate allocation failure`` |
| `xs[::0]` | `slice step cannot be zero` |
| `xs[::k]` with `k: i64` | ``slice step must be `NonZero[i64]`, not `i64` `` |
| `&xs[::2]` | `a slice view selects adjacent elements, so it takes no step` |
| `&mut s[1:3]` on a `str` | ``a `str` is immutable, so a substring view is shared only; write `&s[a:b]` `` |
| `def f(xs: &list[T]) -> &[T]` | `a function cannot return a slice view yet (backlog B11)` |
| `f(&s[1:])` for `f(s: &str)` | ``this argument is a &substr view, but the parameter takes &str; declare the parameter `&substr` to borrow it without copying, or pass an owned copy, `s[a:b]?` `` |
| `total(xs)` for `total(v: &[i64])` | ``this parameter takes a slice view: pass `&xs` for a whole list or buffer, or `&xs[a:b]` for part of it`` |
| `x in v` for a view `v` | ``slice views do not support `in` yet`` |
| `xs[1:3] = ys` | `slice assignment is not supported; assign elements by index, or build a new list` |
| `m[1:2, 0]` | `a slice cannot have several dimensions` |
| `t[n:]` on a tuple | `tuple slice bounds must be integer literals; the result type depends on them` |
| `t[1:]?` on a tuple | ``a tuple slice `t[a:b]` does not allocate, so it takes no `?`; write `t[a:b]` `` |
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

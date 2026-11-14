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
| `v.reverse()` | Reverse the view's elements in place; needs an exclusive view |
| `v.get(i)` | `Option[T]` copy of element `i`, `Nothing` outside the view; non-affine elements only |
| `x in v`, `x not in v` | Whether an element equals `x` |
| `v.count(x)`, `v.find(x)`, `v.rfind(x)` | Number of equal elements, and the first or last matching position within the view as `Option[i64]` |
| `list(v)?`, `v[a:b]?` | An owned `list[T]` of copies of the elements, or of the selected ones |
| `&v[i]`, `&mut v[i]` | Reference to element `i` |
| `&v[a:b]`, `&mut v[a:b]` | Narrower view; bounds count from the start of `v` |
| `for x in v` | `&T` per element, or `&mut T` for an exclusive view; `reversed(v)` and `enumerate(v)` also work |
| `iter(v)` | A [pipeline](../09-iteration/06-pipelines.md) over the elements, with the items of `for x in v` |
| `v == w`, `v != w` | Equal lengths and equal elements; both sides are views of one element type |
| `print(v)`, `str.repr(v)`, `f"{v}"` | The elements as a list, `[2, 3]`; a `&[u8]` as a `bytes` literal, `b"bc"` |

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
itself keeps its own spelling. [Operations on views](#operations-on-views)
compares a view's methods with a list's. Byte views additionally support
[binary string operations and membership](../../stdlib/bytes-stringlike.md).

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

### Operations on views

A view reads and reorders its elements as a list does, but its length is
fixed: a method that would add or remove elements belongs to the owner.

| Operation | `list[T]` | `&[T]` | `&mut [T]` |
| --- | --- | --- | --- |
| `len`, `v[i]`, `&v[a:b]`, `for`, `iter`, `==`, `print` | yes | yes | yes |
| `v[i] = x`, `swap`, `reverse` | yes | read-only | yes |
| `in`, `count`, `find`, `rfind` | yes | yes | yes |
| `list(v)?`, `v[a:b]?` | yes | yes | yes |
| `v[a:b:k]?` with a step | yes | copy from the owner | copy from the owner |
| `sort`, `sort_by_key`, `sort_stable_by_key` | yes | read-only | yes |
| `sorted(v)?` | yes | copy with `list(v)?`, then sort | copy with `list(v)?`, then sort |
| `append`, `extend`, `insert`, `pop`, `clear`, `reserve` | yes | length is fixed | length is fixed |
| `get` | yes | yes | yes |
| `get_ref` | yes | yes | yes |
| `get_mut` | yes | read-only | yes |

`x in v`, `v.count(x)`, `v.find(x)`, and `v.rfind(x)` compare `x` with each
element using the element type's `==`, so they take the element types a list's
[membership](../../facilities/01-builtins/lists.md#lookup-and-removal) takes:
numbers, `bool`, strings, classes, enums, tuples, nested lists, and unions of
them. A list's `count`, `find`, and `rfind` take the same set. Floats use IEEE
equality, so NaN never matches and signed zeros do. Positions count from the
start of the view. `in` evaluates `x` before
the view, as list membership does, and the methods evaluate the receiver
first; `x` is borrowed, as a `&T` argument is, and the search allocates
nothing.
Each compiles to an ordinary loop over the view, specialized for its element
type.

`list(v)?` and `v[a:b]?` copy elements as a list's
[copying slice](../../facilities/01-builtins/lists.md#slices) does: each
element with `copy()`, and each `Shared` handle with `share()`. They reserve
storage for every element first, then copy in order; an element type that
`copy()` rejects is a compile error. `v[a:b]?` takes the view's bounds rules
and no step; copy every k-th element from the owner, `xs[a:b:k]?`. On a
`&[u8]` view, `v[a:b]?` is rejected, since the owner may be a list or a
`Bytes` buffer: write `list(&v[a:b])?` for a list. `copy(v)` and `set(v)` are
rejected with a pointer to `list(v)?`.

`v.get(i)` returns `Some` copy of element `i`, or `Nothing` outside the view,
with [`list.get`](../../facilities/01-builtins/lists.md#lookup-and-removal)'s
contract: a negative index counts from the view's end, and the element type
must not be affine; borrow an affine element with `v.get_ref(i)`.

`v.get_ref(i)` and, on an exclusive view, `v.get_mut(i)` lend element `i` in
place with the contracts of
[`list.get_ref` and `list.get_mut`](../../facilities/01-builtins/lists.md#lookup-and-removal):
every element type, a negative index counts from the view's end, and the result
is `Nothing` outside the view instead of trapping as `&v[i]` does. The index is
observed before the view. The result holds a loan of the whole view, shared for
`get_ref` and exclusive for `get_mut`, and so of the view's owner, until its
last use. The lookup compiles inline to one bounds comparison, with no runtime
call or allocation.

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut rows = [[1]?, [2]?, [3]?]?
    tail = &mut rows[1:]
    match tail.get_mut(-1):
        case Some(row):
            row.append(30)?
        case Nothing:
            pass
    print(tail.get_ref(0), tail.get_ref(2).is_some())?
    print(rows)?
    Ok(())
```

```output
Option[&list[i64]].Some([2]) False
[[1], [2], [3, 30]]
```

`v.reverse()` reorders an exclusive view's elements with `list.reverse`'s
contract: every element type, and no allocation, copy, retain, or drop.

```plenty
class Point:
    x: i64
    y: i64

def main() -> Result[(), AllocError | IoError]:
    mut xs = [4, 1, 2, 1, 3]?
    tail = &xs[1:]
    print(1 in tail, tail.count(1), tail.find(1), tail.rfind(1))?
    middle = tail[1:3]?
    print(middle, list(&xs[3:])?)?
    (&mut xs[1:]).reverse()
    print(xs)?
    points = [Point(0, 0), Point(1, 2)]?
    print(Point(1, 2) in &points[1:])?
    Ok(())
```

```output
True 2 Option[i64].Some(0) Option[i64].Some(2)
[2, 1] [1, 3]
[4, 3, 1, 2, 1]
True
```

`v.sort()`, `v.sort_by_key(key)` and `v.sort_stable_by_key(key)?` order an
exclusive view's elements with the list
[sorts](../../facilities/01-builtins/lists.md#reordering-and-extending)'
contracts: the same element types, key callbacks, positional descending flag
and results. The unstable sorts allocate nothing; the stable sort reserves its
scratch before moving any element, so an `AllocError` leaves the view
unchanged. Elements outside the view stay in place. Each calls the runtime
sort that the list uses, so a view sort runs the same code as sorting a list of
the same elements. A shared view is read-only and has no sorts. `sorted`
takes a list, not a view: copy the view with `list(v)?` and sort the copy.

```plenty
def length(word: &str) -> i64:
    len(word)

def main() -> Result[(), AllocError | IoError]:
    mut scores = [9, 4, 7, 1, 8]?
    (&mut scores[1:4]).sort()
    print(scores)?
    (&mut scores[:]).sort(True)
    print(scores)?
    mut words = ["kiwi", "plum", "fig", "pear", "date"]?
    (&mut words[1:]).sort_stable_by_key(length)?
    print(words)?
    Ok(())
```

```output
[9, 1, 4, 7, 8]
[9, 8, 7, 4, 1]
["kiwi", "fig", "plum", "pear", "date"]
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
including a generic `&[T]`, and annotated locals.

A shared view parameter also lends a bare owned argument, as a `&T` parameter
does: `total(xs)` is `total(&xs)`. This covers lists, `Bytes` and `bytes`
values, `Shared` handles, projected owners such as `bag.items` or `*r`, and
call or `?` results such as `total(make()?)`,
which are held to the end of the statement. An `&mut [T]` parameter still
needs `&mut xs` or an existing exclusive view; exclusive access is never
inferred. A bare `str` is not lent as bytes; pass `s.as_bytes()`. For
example:

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
    print(total(xs)?, total([4, 5]?)?)?
    Ok(())
```

```output
6 5 6 3
6 9
```

`&xs[:]` keeps working. A `&T` type parameter takes a reference to a whole
value, not a view. Standard-library operations that read bytes take
[`&[u8]`](../../stdlib/bytes.md#byte-views), so `buffer.extend(&other[2:])` and
`writer.write(b"ok")` pass a view.
A function with a view parameter may return a reference into it: the first
reference or view parameter is the
[origin](../05-ownership/04-returned-references.md) of a returned reference.
Views may be passed to [scoped workers](../../facilities/03-concurrency/index.md).

A function may return a view under the
[returned-reference origin rule](../05-ownership/04-returned-references.md#returning-views).
A single `Option` or `Result` success payload may carry it under the same
rule. A view cannot be stored in a class field, tuple, collection, or user enum
payload, or captured by an anonymous function. Store an owned copy or bounds
when the data must outlive its owner's loan (backlog B11).

### Substring views

`&s[a:b]` on a `str` borrows the selected text in place as a `&strview`
(issue #595). Like other views it copies nothing, allocates nothing, and takes
no `?`. Its bounds are those of the copying slice `s[a:b]?`: they count
Unicode scalars, a negative bound counts from the end, and bounds outside the
text clamp. A view therefore always starts and ends on a scalar boundary, and
`s[a:b]? == &s[a:b]` for any `a` and `b`.

```plenty
def vowels(text: &strview) -> Result[i64, OverflowError[i64]]:
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

`&strview` is a separate type from `&str`. A `&str` refers to a whole `str`,
whose [representation](../../toolchain/02-architecture/values.md#strings)
stores its byte length and scalar count ahead of the text, and every `str`
operation reads them. A run inside a string has no such prefix, so a view
holds the run's address and byte length instead. `strview` is only written as
`&strview`; there is no owned `strview` and no `&mut strview`, since a `str` is
immutable. The original spelling `&substr` remains a fully compatible alias;
annotations using either spelling denote the same type. Diagnostics use
`&strview`. Taking `&s` without a view annotation still produces an ordinary
`&str`, including in generic `&T` code. A `&strview` parameter also takes a
whole string: `f(s)` implicitly borrows it, and `f(&s)`, a `&str` binding, or a `str` literal work too. Each
converts as `&s[:]` would, without copying, retaining, or allocating. Owned
temporary arguments are evaluated once and stay alive through the containing
full expression, including later arguments and error propagation. An annotated
local still needs a borrow, such as `whole: &strview = &s`, or a literal;
implicit borrowing applies to calls.
The reverse does not hold: a `&str` parameter cannot take a `&strview`
without a copy. Declare the parameter `&strview` to accept both a view and a
whole string, or pass an owned copy, `f(s[a:b]?)`, which the call lends as a
temporary.

| Operation | Meaning |
| --- | --- |
| `len(v)` | Number of scalars; scans the view's text, as positions do |
| `v.byte_len()` | Number of UTF-8 bytes; constant time |
| `v.as_bytes()` | Shared `&[u8]` over the selected UTF-8 bytes, without copying, allocating or scanning |
| `v[i]` | Scalar `i` as a one-scalar `str`; a negative index counts from the end, and an index outside the view traps |
| `&v[a:b]` | Narrower view; bounds count scalars from the start of `v` |
| `v[a:b:step]?`, `v.slice(a, b)` | An owned scalar slice, `Result[str, AllocError]`; copying slices support a step |
| `for c in v` | Each scalar as a one-scalar `str`; `reversed(v)`, `enumerate(v)`, and `iter(v)` also work |
| `v == w`, `v != w`, `<`, `<=`, `>`, `>=` | Compare by Unicode scalar value, as `str` does; either side may be a `str` or a `&strview` |
| `needle in text`, `needle not in text` | Substring membership; either operand may be a `str` or a `&strview` |
| `v.startswith(x)`, `v.endswith(x)` | Exact prefix or suffix; `x` is a `str` or a `&strview` |
| `v.find(x)`, `v.rfind(x)`, `v.count(x)` | First or last scalar position (`Option[i64]`), or nonoverlapping match count |
| `v.get(i)`, `ord(v)` | Optional scalar string at `i`, or the scalar value when the view contains exactly one scalar |
| `v.isascii()`, `v.isspace()`, `v.isdigit()`, `v.isalpha()`, `v.isalnum()` | The same predicates as on `str`; letter/digit predicates are ASCII, whitespace follows Unicode |
| `v.lower()`, `v.upper()`, `v.strip()`, `v.lstrip()`, `v.rstrip()` | Owned transformed text, `Result[str, AllocError]` |
| `v.concat(x)`, `v + x`, `x + v`, `v.repeat(n)` | Owned concatenated or repeated text, `Result[str, AllocError]` |
| `v.replace(old, new)`, `v.removeprefix(x)`, `v.removesuffix(x)` | Owned replacement or removal result; all text arguments accept strings or views |
| `v.split()`, `v.split(sep)`, `v.splitlines(keepends=False)` | Owned fields with the same separators, empty-field rules and line boundaries as `str` |
| `v.join(parts)` | Owned joined text; `parts` may be an existing iterable of `str` or a text view |
| `T.parse(v)` | Typed integer or floating-point parsing, with the same `ParseError` results as `str` |
| `print(v)`, `f"{v}"` | The text itself; `str.repr(v)` and `f"{v!r}"` quote it as a `str` |

Each scalar a loop or index yields fits in a string's inline form, so these
operations allocate nothing. Finding a bound, `len(v)`, `v[i]`, and searches
scan the selected text; an owning `str` caches its scalar count, a view does
not. Cache `len(v)` locally when a loop needs it repeatedly.

All text methods above accept either strings or views in their text operands,
including methods called on an owning string. They read the selected UTF-8
range directly and allocate only an owned result when required. A view cannot
stand in for an owning `str` argument to an arbitrary function, or be stored in
a collection. It may be returned under the
[origin rule](../05-ownership/04-returned-references.md#returning-views).
Joining collections or generators of views therefore
remains unsupported. String repetition uses `repeat(n)?`; `*` is rejected for
both strings and views. `str.from` converts numeric and boolean values, rather
than copying text, and there is no `str.format` method: formatting uses f-strings
and `std.fmt.Formatter`.

The byte view returned by `as_bytes()` keeps the text view's original loan,
including inline bytes stored in a string slot. It can be bound locally, passed
to any `&[u8]` reader, narrowed by byte positions, indexed, and iterated as an
ordinary byte view. It grants no mutable access to string storage. A function
may return this byte view when it borrows from its first reference or view
parameter, just as it may return a narrowed slice view.

A substring view borrows its string as other views borrow their owner: while
it is live, the string cannot be reassigned or moved. Returning the view keeps
that loan in the caller; storing it in a class or collection remains unsupported
(backlog B11).

## Limits

| Source | Diagnostic |
| --- | --- |
| `ys: list[i64] = xs[1:]` | ``slice returns Result[list[i64], AllocError]; use `?` to propagate allocation failure`` |
| `xs[::0]` | `slice step cannot be zero` |
| `xs[::k]` with `k: i64` | ``slice step must be `NonZero[i64]`, not `i64` `` |
| `&xs[::2]` | `a slice view selects adjacent elements, so it takes no step` |
| `&mut s[1:3]` on a `str` | ``a `str` is immutable, so a substring view is shared only; write `&s[a:b]` `` |
| `f(&s[1:])` for `f(s: &str)` | ``this argument is a &strview view, but the parameter takes &str; declare the parameter `&strview` to borrow it without copying, or pass an owned copy, `s[a:b]?` `` |
| `width(s)` for `width(v: &[u8])` and `s: str` | ``this parameter takes a slice view; a `str` is text, so pass `s.as_bytes()` for its UTF-8 bytes`` |
| `fill(xs)` for `fill(v: &mut [i64])` | ``this parameter takes a slice view: pass `&mut xs` for a whole list or buffer, or `&mut xs[a:b]` for part of it`` |
| `v.append(x)` on a view | `` `append` changes a list's length, and a slice view's length is fixed; call it on the list after the view's last use `` |
| `v.sort()` on a shared view | ``a shared slice view is read-only, so it has no `sort`; take an exclusive view with `&mut xs[a:b]` `` |
| `sorted(v)` on a view | ``sorted takes a list, not a slice view: copy the view with `list(v)?` and sort the copy`` |
| `v[a:b]?` on a `&[u8]` | ``a copying slice of &[u8] could be a `list[u8]` or a `Bytes` buffer; write `list(&v[a:b])?` for a list, or `extend` an empty `Bytes` with `&v[a:b]` `` |
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

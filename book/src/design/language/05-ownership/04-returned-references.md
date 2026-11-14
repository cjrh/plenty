# Returned references

A function or method returning `&T`, `&mut T`, `&[T]`, `&mut [T]`, or
`&strview` borrows its result from its first reference parameter, the *origin*.
A [slice view](../04-expressions/slices.md#slice-views) parameter, `&[T]` or `&mut [T]`, counts as a reference parameter, as does a
by-value [`Generator[&T]`](../09-iteration/03-generators.md#reference-items)
parameter, whose items borrow the caller's data rather than the generator.
A function without a reference parameter cannot return a borrow, and a
mutable result requires an `&mut` origin. Every return path must originate in the origin; later reference
parameters, local owners, and owned parameters cannot escape. Direct parameter
references, reborrows, field and element projections, borrowed-match payloads,
and forwarding calls are supported. Each arm of a returned `if`, `match`, or
inline conditional must originate in the origin.

```plenty
class Pair:
    first: i64
    second: i64

def first(pair: &Pair) -> &i64:
    &pair.first

def main() -> Result[(), IoError]:
    pair = Pair(3, 4)
    value = first(&pair)
    print(*value)?
    Ok(())
```
```output
3
```

An element or view borrows the storage it projects from. References used only
to compute its index or bounds do not become its origin: `&values[index(other)]`
borrows `values`. Returning the following element is therefore rejected even
though the index computation borrows the first parameter:

```plenty-error
def index(values: &[i64]) -> i64:
    0

def wrong(origin: &[i64], later: &[i64]) -> &i64:
    &later[index(origin)]

def main() -> ():
    pass
```
```error
returned reference must originate from the first reference parameter
```

An element projection from named storage evaluates its computed indices once,
in source order, before taking an element address. An exclusive element borrow
can therefore read its owner to compute an index. No element address is kept
across those computations, so an index helper cannot resize storage behind a
saved address.

Later reference parameters are ordinary inputs. Their loans end when the call
returns, so a temporary key or a borrow of another owner does not restrict the
result. For a method, the origin is its reference receiver:

```plenty
class Scores:
    names: list[str]
    points: list[i64]

    def points_of(self: &mut Scores, name: &str) -> &mut i64:
        for i, candidate in enumerate(&self.names):
            if candidate == name:
                return &mut self.points[i]
        &mut self.points[-1]

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut scores = Scores(["ada", "bob"]?, [3, 5]?)
    mut name = "bob"
    points = scores.points_of(&name)
    name = "eve"
    *points = *points + 10
    print(scores.points)?
    print(name)?
    Ok(())
```
```output
[3, 15]
eve
```

This small lookup assumes equally sized, nonempty lists and uses the last
score as its fallback. Enumeration avoids manual index arithmetic while
keeping the reference-returning method infallible. A general lookup can return
an [optional reference](05-optional-references.md) instead.

To return a borrow of a different argument, make it the first reference
parameter. A function cannot choose between two arguments; that relationship
needs annotations Plenty does not have. Signatures alone fix the origin, so
`Callable` types and generic functions follow the same rule.
Capturing and one-shot closures cannot return loan-carrying values, including
views and optional/fallible borrows; their environments do not support returned
loans. Boxed callables have the same return restriction.

The result extends its origin loan until its last use. A body that directly
returns its parameter or a fixed field projection permits a precise returned
field loan; the original call still requires access to its whole argument.
Indexed origins, control-dependent returns, and other bodies conservatively
protect the whole argument, even after further projection.

A `&mut T` result can be assigned through directly, without a binding:
`*f(&mut xs) = value` and `*f(&mut xs) += value` call `f` once and write its
referent. The call runs first, as in `r = f(&mut xs); *r = value`, so its loan
is live through the right-hand side: `*f(&mut xs) = xs[1]` is a conflicting
borrow. Read the value into a local first.

```plenty
def head(xs: &mut [i64]) -> &mut i64:
    &mut xs[0]

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut xs = [1, 2, 3]?
    *head(&mut xs[:]) = 9
    *head(&mut xs[1:]) += 10
    print(xs)?
    Ok(())
```
```output
[9, 12, 3]
```

A caller may bind a result from named storage and reborrow it. A temporary
origin survives only its full expression, the statement or condition that
evaluates it. A shared temporary passed to an ordinary function follows this
rule: `len(view("text"))` can use a returned `&str`, but `r = view("text")` is
rejected even when `r` is unused. A temporary method receiver, such as
`make().label()`, follows the same rule: the result may be used within the
statement, and the receiver drops at its end. Binding the result, or returning
it, is rejected:

```plenty
class Point:
    x: i64
    name: str

    def label(self) -> &str:
        &self.name

def make(name: str) -> Point:
    Point(0, name)

def main() -> Result[(), IoError]:
    print(make("origin").label())?
    print(len(make("east").label()))?
    Ok(())
```
```output
origin
4
```

A temporary receiver that itself borrows other storage, such as a returned
`&Point` in `pick(&a).view()`, cannot lend a returned reference: its loan of
that storage would end before the result is used. Bind the receiver first,
`p = pick(&a)`, then call `p.view()`.

A method on an element or field of a temporary, as in
`[make("a")]?[0].label()` or `holder.items()[0].view()`, borrows that element
in place. An owned temporary lives to the end of the statement, as a
temporary receiver does; a returned reference lends as a named reference does.

Returned borrows cannot enter general aggregates, scoped workers with temporary
origins, or generator suspension. No lifetime annotation or callee-body
inference changes the signature's first-reference origin.

A borrow that may be absent or may fail is returned as `Option[&T]` or
`Result[&T, E]`, including view payloads such as `Option[&[T]]`, under the same
origin rule; see
[optional and fallible references](05-optional-references.md).

## Returning views

Returning a view copies its pointer and length, without allocating or copying
its elements. Narrowing a view parameter keeps the original owner's loan;
it does not borrow a descriptor local to the callee. The owner cannot move,
be reassigned, or resize until the returned view's last use. An exclusive
returned view also prevents competing access through its origin. Indexing
retains its bounds checks.

```plenty
def tail(values: &[i64]) -> &[i64]:
    &values[1:]

def ending(text: &strview) -> &strview:
    &text[-2:]

def main() -> Result[(), AllocError | IoError]:
    values = [10, 20, 30]?
    selected = tail(values)
    word = "café"
    text = ending(word)
    print(len(selected), selected[0], text)?
    Ok(())
```
```output
2 20 fé
```

The same rule allows `-> &mut [T]` from an exclusive origin and `-> &[u8]`
from an origin's `as_bytes()` borrow. Inline string bytes remain protected
by their owning string slot's loan. A local owner or temporary cannot escape
through a returned view. A result borrowing a temporary argument may be used
only within that argument's full expression, following the temporary rules
above. View fields in classes and other general stored borrows remain
unsupported.

# Generators

A function containing `yield` declares `Generator[T]`. Ordinary call syntax
`producer(arguments)` constructs its owned frame; `.new(arguments)` is an
equivalent alias. Arguments evaluate first and copy or move into the frame.
Construction runs no body code and requires no heap allocation. Argument
expressions keep their own allocation and error contracts.

```plenty
def numbers(stop: i64) -> Generator[i64]:
    mut value = 0
    while value < stop:
        yield value
        # value starts at zero and value < stop <= i64.MAX.
        unchecked:
            value = value + 1

def main() -> Result[(), IoError]:
    for value in numbers(3):
        print(value)?
    Ok(())
```
```output
0
1
2
```

Resumption executes until a statement-only `yield value`, bare `return`, or
fallthrough. Yield types are exact and owned payloads transfer to the caller.
Reference, generator and closure yields are rejected; unit `()` yields are
ordinary values. A generator body cannot return a value. An ordinary factory without `yield` may return another
frame by moving it. `?` is unsupported in a generator body; match fallible results
explicitly. Recoverable arithmetic failures cannot propagate from a generator:
handle a
local [`checked:` result](../03-types/numbers.md#checked-blocks), call a helper
that returns a `Result` and yield or match it, or use a small `unchecked:` suite
with a documented bound.

`next(&mut g) -> Option[T]` borrows a mutable generator owner or reborrows an
exclusive generator reference. `next(g)` is shorthand for named bindings and
exclusive references. Shared references, immutable owners, and mutable
temporaries cannot be advanced. The borrow lasts only through the call;
exhaustion is stable and produces `Nothing` on every later call. Resumption
and its Option wrapper add no allocation beyond body operations.

Loops, comprehensions, and iterable collection constructors consume frames.
`break` destroys a loop's hidden frame without executing subsequent generator
statements.

## Lending a generator

`for value in &mut g` advances a mutable owner in place, and `for value in g`
does the same through an exclusive `g: &mut Generator[T]` parameter. Items are
owned, as from `next`. The loop never drops the frame: after `break`, `return`
or `?`, the owner keeps the remaining items for a later `next`, loop or owning
consumer, and drops the suspended frame at its own scope end. Comprehensions,
`enumerate`, `zip` and `iter` accept `&mut g` the same way. A `zip` that
stops because a later input ended has already taken one more item from an
earlier generator; that item is dropped. Pass a lent generator as the last
`zip` input to keep every item.

```plenty
def numbers(stop: i64) -> Generator[i64]:
    mut value = 0
    while value < stop:
        yield value
        # value starts at zero and value < stop <= i64.MAX.
        unchecked:
            value = value + 1

def skip_small(g: &mut Generator[i64]) -> Option[i64]:
    for value in g:
        if value >= 2:
            return Some(value)
    Nothing

def main() -> Result[(), IoError]:
    mut g = numbers(5)
    print(skip_small(&mut g))?
    for value in &mut g:
        print(value)?
    print(next(g))?
    Ok(())
```
```output
Option[i64].Some(2)
3
4
Option[i64].Nothing
```

The exclusive loan lasts for the whole loop, so `next(&mut g)` or another use
of `g` inside the body is a conflicting borrow. A shared `&g` cannot advance,
and `reversed(&mut g)` is rejected. Inside a generator body the loan cannot
cross `yield`; consume the owned frame with `for value in g` or call `next`
between yields instead. A lent loop adds no allocation.

A `&mut Generator[T]` or `Generator[T]` argument can be passed by
[name](../06-functions/01-signatures-and-calls.md) in any order, and to a
function whose defaults are omitted, like any other argument. Arguments run
in written order, so `take(g=&mut g, x=next(g).unwrap_or(0))` is a
conflicting borrow.

## Frame ownership and identity

Assignment, arguments, and returns transfer ownership. Standard Option/Result
wrappers may own frames, including nested wrappers, and remain affine. Frames cannot enter collections, class fields, tuples, user enum
payloads, or heap-stored closures. Printing, comparison, length, and membership
are undefined for frames.

Each producer and concrete specialization has a distinct frame identity.
`Generator[T]` describes the yield contract without erasing that identity.
Local annotations preserve the initializer's producer, and parameters,
references, aliases, and standard sum wrappers specialize to actual frames.
Different producers with the same item type may meet wherever a value is
chosen, and their types then join:

- a conditional expression, `a if c else b`, wherever it appears;
- an `if` or `match` value, such as `g = match n: ...`, wherever it appears,
  including in a generator body; branches that exit with `return`, `break`
  or `continue` take no part;
- a factory, a function without `yield` that returns `Generator[T]`, through
  its `return` statements, a tail `if`/`else`, or a tail `match`;
- reassignment of a local, such as `mut g = one()` then `g = two()`,
  including inside a loop; the local's type is the join of every producer
  assigned to it;
- an `Option` or `Result` value at any of those positions, at any depth of
  standard `Option`/`Result` wrappers: one built there, such as
  `Some(one()) if c else Some(two())` or a factory returning
  `Option[Generator[T]]`, or one already built, such as a call result or a
  parameter (issue #900). A built value's success payload moves into the
  joined storage; its tag and any `Err` payload stay as they are.

```plenty
def one() -> Generator[i64]:
    yield 1

def two() -> Generator[i64]:
    yield 2
    yield 3

def pick(flag: bool) -> Option[Generator[i64]]:
    if flag:
        return Some(one())
    Some(two())

def main() -> Result[(), AllocError | IoError]:
    mut g = one()
    g = two()
    print(g.count())?
    match pick(False):
        case Some(chosen):
            print(chosen.count())?
        case Nothing:
            pass
    for flag in [True, False]?:
        o = pick(flag) if flag else Some(two())
        match o:
            case Some(chosen):
                print(chosen.count())?
            case Nothing:
                pass
    Ok(())
```

```output
2
2
1
2
```

The joined frame is inline storage sized for the largest member, and each
member's frame is copied into it where the join happens. Frames already
describe their own resume, drop and relocation, so the join needs no tag and
allocates nothing; nested joins flatten into one set of members. `Nothing` or
`Err` alone cannot infer an absent producer. No hidden boxing makes frames
interchangeable.

| Position | Matching `Generator[T]` item type suffices | Producer identity must match |
|---|---|---|
| Parameter `Generator[T]` or `&mut Generator[T]` | Yes: each producer gets its own specialization | |
| `for`, `next`, comprehension, `iter(...)` source | Yes | |
| Conditional expression `a if c else b` | Yes: the producers join | |
| `if`/`match` value, such as `g = match n: ...` | Yes: the producers join | |
| Factory return (`return`, tail `if`/`else`, tail `match`) | Yes: the producers join | |
| Reassignment of a local | Yes: the producers join | |
| An `Option`/`Result` value at a joining position, built there or already built | Yes: the producers join | |

When two different producers meet where one identity is required, the
diagnostic names both and lists where producers join. There is no erased
generator: every join above is a fixed set of known members, so
none needs heap allocation or dynamic dispatch.

A join is found while checking the function: when a value's type was fixed
before a different producer met it, the function is checked again with the
joined type at that value. Only that function is checked again, and the
cost is counted in passes over it:

- Conditional expressions and reassignments: one pass records all of them,
  so any number of them costs one extra pass.
- `if` and `match` values and a factory's returns: each value whose earlier
  branches or returns fixed a narrower type costs one more pass, because
  checking stops there.
- Nested choices: a join that widens an enclosing choice may cost one more
  pass for that choice.

A suspended frame retains its live owned locals. Completion releases them;
dropping a suspended or never-started frame releases remaining locals and
captures without resuming the body or allocating. Short local borrows completed
within one resume are permitted; no loan of the frame's own locals may cross
`yield`. `yield` inside `with` is unsupported. There is no yield-from,
send/throw, or async/await.

### Borrowing frames

A generator can take `&T`, `&mut T` and view parameters. Its frame reads the
caller's data through them, and a loan through one may cross `yield`:

```plenty
def evens(xs: &list[i64]) -> Generator[i64]:
    for x in xs:
        if *x % 2 == 0:
            yield *x

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    mut g = evens(&xs)
    print(next(g))?
    for v in g:
        print(v)?
    xs.append(6)?
    print(evens(&xs).count())?
    Ok(())
```
```output
Option[i64].Some(2)
4
3
```

The call result keeps every argument loan until the generator's last use, as
a returned reference keeps its origin. Changing, moving or dropping `xs` while
`g` may still resume is a borrow error; once `g` is exhausted, dropped or no
longer used, `xs` is free again. A `&mut` argument stays exclusively borrowed
for the same span. A function may return such a generator when it borrows only
through that function's own reference or view parameters; returning one that
borrows a local or an owned parameter is rejected. A conditional expression or
an `if`/`match` value that chooses between borrowing generators keeps every
arm's loans, and a returned choice checks each arm; an arm producing an
owned generator contributes no loan. A `mut` binding that holds an owned
generator cannot be assigned a borrowing one; bind it to a new name.

A borrowing generator can be bound, advanced, iterated, consumed by a
fluent terminal, passed by value to a function, or returned. A generator
passed by value keeps its loans until the call returns, so a later argument
cannot move or exclusively borrow the data it reads. It cannot be stored
where its loan would be lost: in an `Option` or `Result` payload, a tuple, a
class field or a by-value closure capture.

A generator parameter may itself be a borrowing generator. The wrapper's frame
keeps the argument's loans, so the caller's data stays borrowed until the
wrapper's last use, and the wrapper may be returned when the wrapped generator
borrows only through the function's reference parameters:

```plenty
def evens(xs: &list[i64]) -> Generator[i64]:
    for x in xs:
        if *x % 2 == 0:
            yield *x

def relayed(g: Generator[i64]) -> Generator[i64]:
    for v in g:
        yield v

def evens_of(xs: &list[i64]) -> Generator[i64]:
    relayed(evens(xs))

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    print(relayed(evens(&xs)).count())?
    xs.append(6)?
    print(evens_of(&xs).count())?
    Ok(())
```
```output
2
3
```

A parameter that carries a reference, such as `Option[&T]` or
`Result[&mut T, E]`, borrows like a reference parameter (issue #913): the
call result keeps the argument's loan, and the frame may hold a reference
from its payload across `yield`.

```plenty
def find(xs: &list[str], key: &str) -> Option[&str]:
    for x in xs:
        if *x == *key:
            return Some(x)
    Nothing

def repeated(o: Option[&str], times: i64) -> Generator[&str]:
    match o:
        case Some(s):
            for _ in range(times):
                yield s
        case Nothing:
            pass

def main() -> Result[(), AllocError | IoError]:
    xs = ["ada", "bob"]?
    for name in repeated(find(&xs, "bob"), 2):
        print(name)?
    print(repeated(find(&xs, "eve"), 2).count())?
    Ok(())
```
```output
bob
bob
0
```

A copy of the parameter, such as `keep = o` or `mut keep = o`, can also be
held across `yield`. A `mut` holder declared empty and assigned the parameter
later (`mut keep: Option[&str] = Nothing` then `keep = o`) cannot: the check
treats the holder as borrowing frame state, and the error is "a borrow of
`keep` cannot remain live across yield". Initialize the holder from the
parameter or use the parameter directly.

A generator never takes a lent pipeline (`&mut Iter[T]`). To pass a pipeline
to a generator, convert it with
[`.generator()`](06-pipelines.md#returning-a-resumable-stream) and pass the
result as `&mut Generator[T]`. Lending copies the pipeline's state into the
borrower and writes it back when the call returns, and a suspended frame has
not returned. A `&mut Iter[T]` generator parameter is an error: "a generator
cannot take a lent pipeline".

### Reference items

A borrowing generator can yield shared references into the data it borrows.
`Generator[&T]` always lends, and each item borrows what the frame borrows:

```plenty
def names(xs: &list[str]) -> Generator[&str]:
    for x in xs:
        yield x

def main() -> Result[(), AllocError | IoError]:
    xs = ["ab", "c"]?
    mut g = names(&xs)
    first = next(g)
    drop(g)
    print(first)?
    for a, b in zip(names(&xs), names(&xs)):
        print(f"{a}{b}"?)?
    Ok(())
```
```output
Option[&str].Some("ab")
abab
cc
```

An item stays valid after the generator advances, is consumed or is dropped:
it borrows the caller's data, not the frame. Changing, moving or dropping
`xs` while an item is live is a borrow error, as for a reference to a list
element. A function may return an item, or a generator of items, when it
borrows only through the function's reference or view parameters.

A by-value `Generator[&T]` parameter can also be the
[origin of a returned reference](../05-ownership/04-returned-references.md):
the function consumes the generator, but its items borrow the caller's data,
which the caller keeps borrowed for as long as the result is live.

```plenty
def names(xs: &list[str]) -> Generator[&str]:
    for x in xs:
        yield x

def longest(g: Generator[&str]) -> Option[&str]:
    mut best: Option[&str] = Nothing
    for v in g:
        match best:
            case Some(b):
                if len(v) > len(b):
                    best = Some(v)
            case Nothing:
                best = Some(v)
    best

def main() -> Result[(), AllocError | IoError]:
    xs = ["ab", "abc", "c"]?
    print(longest(names(&xs)))?
    Ok(())
```
```output
Option[&str].Some("abc")
```

A yielded reference must borrow through a shared reference, view,
`Option[&T]` or `Result[&T, E]` parameter, or be an item of a generator that
does, or of a generator parameter. Yielding a reference to a local, an owned
parameter or data behind a `&mut` or `Option[&mut T]` parameter, including an
item of a generator over that data, is rejected with "a yielded reference must
borrow through the generator's shared reference, view, `Option[&T]` or
`Result[&T, E]` parameters, not the generator's own state".
A generator body may hold an item it took with `next` through a
`&mut Generator[&T]` parameter across `yield` and yield it again (issue #922):
the item borrows the caller's source, which stays borrowed while the frame
lives.
`Generator[&mut T]` is rejected with "generators cannot yield mutable
references".

An item of a by-value `Generator[&T]` parameter may likewise be held across
`yield`, whether taken by `for` or by `next` on a `mut` local the parameter
was moved into (issue #926).

Items follow the storage rules for references: they cannot be collected by
`list`, `set` or `dict`, stored in a class field or collection, or captured
by an anonymous function. Copy an item to keep it beyond its source's borrow.

Limits:

- An item keeps every loan of its frame, including a `&mut` argument that
  the item does not read, so while any item is live that argument cannot be
  written or borrowed with `&mut`. It can be read once the generator is no
  longer used.
- An item returned by `next` through a `&mut Generator[&T]` parameter keeps
  the generator borrowed, not only its source.
- `.generator()`, comprehensions, `sorted`, `join` and `extend` reject
  reference items.
- A generator cannot be assigned through a `&mut Generator[T]` reference when
  it borrows; advance it with `next`, or assign a new generator to the owning
  binding.

## Fluent transformations

Generators expose the [pipeline adapters and terminals](06-pipelines.md):
`numbers(100).filter(even).map(square).fold(0, add)` consumes the generator
through a fused adapter loop. Source resumption retains its ordinary cost;
the fluent adapters do not each construct another suspended frame.

A pipeline can become a returnable resumable stream through explicit
`.generator()`. It transfers already prepared source and capture state into
one frame without advancing any item. The source may be owned or borrowed: a
borrowed local, a reference or view parameter, a `&mut` source, or a
reference binding makes a [borrowing frame](#borrowing-frames) that keeps the
source borrowed, shared or exclusive as the pipeline borrows it. Callback
captures must be owned. There is
no implicit conversion at a `Generator[T]` parameter or return annotation. The
resulting frame follows the specialization, suspension and cleanup rules above.

### Pipelines in generator bodies

A generator body uses [pipelines](06-pipelines.md) as other functions do: it
can loop over one, bind one to a local, and advance a binding with `next`,
with or without `yield` while the pipeline is live (issue #924). A suspended
pipeline's source, callbacks and progress are frame state; dropping the frame
drops the items it has not produced.

```plenty
def larger(x: &i64) -> bool:
    *x > 1

def large(xs: &list[i64]) -> Generator[i64]:
    for x in iter(xs).filter(larger):
        yield *x

def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    for x in large(&xs):
        print(x)?
    Ok(())
```
```output
2
3
```

A pipeline's loans follow the rule for references: one rooted at a reference
or view parameter may cross `yield`, while one borrowing a local of the frame
may not. Move an owned source into the pipeline, as `iter(xs)`, or consume
the pipeline with a terminal before the next `yield`.

```plenty-error
def walk() -> Generator[i64]:
    xs = [1, 2].unwrap()
    for x in iter(&xs):
        yield *x

def main() -> ():
    drop(walk())
```
```error
a borrow of `xs` cannot remain live across yield
```

## Generator expressions

`(expression for name in source if condition)` creates a concrete owned
`Generator[T]`, with its yielded type inferred from `expression`. Multiple
`for` and `if` clauses have the same scopes and nesting limits as comprehensions.
The outer source evaluates once at creation; inner sources, filters, and the
element expression run only as the frame advances. A range outer source is
stored in the frame and advances as a [counted loop](01-iterables.md). A sole call argument can
omit parentheses: `list(n for n in range(4))`. Arithmetic in an element or
filter follows the same generator restriction; a Result-returning helper may
produce Result items. With other arguments,
parenthesize the generator expression.

Inputs and referenced enclosing local values copy or move into the frame at
creation. Copyable captures are snapshots; later assignments do not change them.
Affine captures become unavailable to the caller. Captures are taken in order
of first use in the remaining clauses, then the yielded expression, after the
outer source. References cannot be captured, including a borrowed outer source;
use an owned source or `copy(source)?`. Adapter sources `enumerate`, `zip`, and
`reversed` evaluate their inputs at creation without allocating an adapter.

Construction and advancement add no heap allocation beyond the expressions
they run. The same yield restrictions, propagation restrictions, destruction,
and concrete-frame specialization rules apply as for generator functions.
Dropping an unstarted frame releases its captures without evaluating filters
or elements; short-circuit consumers destroy the remaining frame immediately.

Recursive inline layouts are rejected. Nested frame layouts are limited to
64 levels, native frame offsets to signed 32-bit sizes, a function's native
stack frame (which holds the inline frames it owns) to Cranelift's 1 GiB, a
compile-time error naming the function, and concrete consumer
specializations to 16,384 per consumer and 65,536 per compilation, with the
dependency depth limit of generic functions guarding expanding recursion (see
[specialization](../../toolchain/02-architecture/emission.md)). These are implementation limits, not
heap-backed fallback behavior. See
[generator implementation](../../toolchain/02-architecture/environments.md).

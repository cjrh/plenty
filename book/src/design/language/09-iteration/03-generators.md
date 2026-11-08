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
earlier generator; that item is dropped.

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
between yields instead. A lent loop adds no allocation. Assignment, arguments, and returns transfer ownership. Standard
Option/Result wrappers may own frames, including nested wrappers, and remain
affine. Frames cannot enter collections, class fields, tuples, user enum
payloads, or heap-stored closures. Printing, comparison, length, and membership
are undefined for frames.

Each producer and concrete specialization has a distinct frame identity.
`Generator[T]` describes the yield contract without erasing that identity.
Local annotations preserve the initializer's producer, and parameters,
references, aliases, and standard sum wrappers specialize to actual frames.
A factory must resolve to one producer per specialization; equal yield types
do not allow different producers to join. `Nothing` or `Err` alone cannot
infer an absent producer. No hidden boxing makes frames interchangeable.

| Position | Matching `Generator[T]` item type suffices | Producer identity must match |
|---|---|---|
| Parameter `Generator[T]` or `&mut Generator[T]` | Yes: each producer gets its own specialization | |
| `for`, `next`, comprehension, `iter(...)` source | Yes | |
| Factory return, `if`/`match` branches, reassignment of one binding | | Yes |
| `Option`/`Result` payload joined across branches | | Yes |

When two different producers meet where one identity is required, the
diagnostic names both and suggests branching inside one generator body.
Joining different producers through an inline sum of their frames or an
explicit erased generator is planned (issue #872).

A suspended frame retains its live owned locals. Completion releases them;
dropping a suspended or never-started frame releases remaining locals and
captures without resuming the body or allocating. Short local borrows completed
within one resume are permitted, but generator parameters cannot be references
and no live loan may cross `yield`. `yield` inside `with` is unsupported.
There is no yield-from, send/throw, or async/await.

## Fluent transformations

Generators expose the [pipeline adapters and terminals](06-pipelines.md):
`numbers(100).filter(even).map(square).fold(0, add)` consumes the generator
through a fused adapter loop. Source resumption retains its ordinary cost;
the fluent adapters do not each construct another suspended frame.

An owned pipeline can become a returnable resumable stream through explicit
`.generator()`. It transfers already prepared source and capture state into
one frame without advancing any item. Borrowed sources or callback captures
are ineligible, including when mapped items are owned. There is no implicit
conversion at a `Generator[T]` parameter or return annotation. The resulting
frame follows the specialization, suspension and cleanup rules above.

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

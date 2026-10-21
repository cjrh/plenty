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
        value = value + 1

def main() -> Result[(), IoError]:
    for value in numbers(3):
        print(value)?
    Ok(())
```

Resumption executes until a statement-only `yield value`, bare `return`, or
fallthrough. Yield types are exact and owned payloads transfer to the caller.
Reference, generator, closure, and unit yields are rejected; a generator body
cannot return a value. An ordinary factory without `yield` may return another
frame by moving it. `?` is unsupported in a generator body; match fallible results
explicitly.

`next(&mut g) -> Option[T]` borrows a mutable generator owner or reborrows an
exclusive generator reference. `next(g)` is shorthand for named bindings and
exclusive references. Shared references, immutable owners, and mutable
temporaries cannot be advanced. The borrow lasts only through the call;
exhaustion is stable and produces `Nothing` on every later call. Resumption
and its Option wrapper add no allocation beyond body operations.

Loops, comprehensions, and iterable collection constructors consume frames.
`break` destroys a loop's hidden frame without executing subsequent generator
statements. Assignment, arguments, and returns transfer ownership. Standard
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

A suspended frame retains its live owned locals. Completion releases them;
dropping a suspended or never-started frame releases remaining locals and
captures without resuming the body or allocating. Short local borrows completed
within one resume are permitted, but generator parameters cannot be references
and no live loan may cross `yield`. `yield` inside `with` is unsupported.
There is no yield-from, send/throw, or async/await.

## Generator expressions

`(expression for name in source if condition)` creates a concrete owned
`Generator[T]`, with its yielded type inferred from `expression`. Multiple
`for` and `if` clauses have the same scopes and nesting limits as comprehensions.
The outer source evaluates once at creation; inner sources, filters, and the
element expression run only as the frame advances. A range outer source is
stored in the frame and advances as a [counted loop](01-iterables.md). A sole call argument can
omit parentheses: `any(n == 2 for n in range(10))`. With other arguments,
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
64 levels, native frame offsets to signed 32-bit sizes, and concrete consumer
specializations to 256 per compilation. These are implementation limits, not
heap-backed fallback behavior. See
[generator implementation](../../toolchain/02-architecture/environments.md).

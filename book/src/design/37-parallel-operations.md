# Explicit parallel operations

Executor methods opt into parallel work. Ordinary loops and comprehensions remain
serial. These methods reuse the pool's fixed workers, bounded queue, and
[worker eligibility checks](32-scoped-native-threads.md).

## Fallible ordered mapping

`pool.map_result(function, inputs)` accepts a named function taking `T` by value
and returning `Result[U, E]`, and an owned `list[T]` or integer `range[T]`.
Generic worker arguments are inferred. `U` must be non-unit; inputs, outputs, and
errors must satisfy the ordinary owned executor-job restrictions.

The return type is `Result[list[U], ParallelError[E]]`. `ParallelError[E]` is an
inline prelude enum with `Allocation(AllocError)`, `Shutdown`, and `Worker(E)`.
Its wrapper needs no allocation, including when `E` contains an inline value.
Unlike `map`, which keeps worker results as list elements, `map_result` extracts
successes and returns one concrete worker error.

Successful outputs follow input order. When several inputs fail, the error from
the lowest input index wins, regardless of completion order. A failed submission
stops further submissions; an error from an earlier accepted input takes precedence
over that submission failure. Allocation failure before any submission returns
`Allocation`; a closed pool returns `Shutdown`, including for empty input.

At most `max_workers + queue_capacity` jobs are outstanding. On observing a
worker error, stop submitting and drain all accepted jobs before returning. Drop
partial outputs, later errors, and unsubmitted inputs exactly once. Already
executed effects are not undone, and some later inputs may have run. Workers
must not depend on every input being scheduled to make progress.

Output capacity and the job window are reserved before starting work. Each
submitted job still needs a fallible cell allocation. Lists are consumed on every
outcome; ranges retain their normal value semantics.

See the [runnable lesson](../tutorial/89-map-fallible-work-in-parallel.md).

## Fixed-tree reduction

`pool.reduce_tree(function, inputs)` consumes a list or integer range. The named
worker takes two elements by value and returns the same type: `(T, T) -> T`.
Generic types are inferred, and ordinary executor eligibility rules apply.
The result is `Result[Option[T], PoolMapError]`: `Nothing` for empty input,
`Some(value)` otherwise. No identity value is inserted or copied.

Each round combines adjacent pairs, preserving left/right order. An odd final
element advances unchanged. The next round begins after the current one completes.
For five inputs the grouping is `f(f(f(a, b), f(c, d)), e)`. Worker count, queue
capacity, and completion order do not change this tree. A nonempty input of length
`n` calls the worker exactly `n - 1` times on success.

This is explicit reassociation, not a left fold. Floating-point sums can differ
from a serial loop; pure workers produce the same result for the same input and
target across pool sizes. Effect order is unspecified, and workers with channel
communication must not depend on another pair being scheduled. Integer overflow
and worker traps retain the ordinary language behavior.

Empty and singleton inputs allocate nothing in the reduction. Larger inputs use
one fallible buffer of type-sized slots and a bounded job window. Pair arguments
move directly into each job cell's inline environment; no tuple allocation or
implicit payload copy is needed. Each round compacts results into the same scratch
buffer. Scratch storage is proportional to input length, even for a range.

Infrastructure failure stops submission, drains accepted jobs, and destroys
intermediate and unsubmitted values. The original list is consumed on every
outcome. A closed pool returns `Shutdown`, including for empty input. Worker
`Result` values, if `T` itself is a `Result`, are ordinary elements; this operation
does not implicitly propagate them or claim a first application error.

See the [reduction lesson](../tutorial/90-combine-values-in-parallel.md).

## Design choice

Rayon documents unspecified selection among parallel errors and unspecified
reduction order. Plenty chooses input-order error selection and a fixed reduction
tree to make outcomes easier to reproduce, at the cost of waiting for earlier
jobs and round boundaries. This does not make external effects deterministic.
[Rayon parallel iterator documentation](https://docs.rs/rayon/latest/rayon/iter/trait.ParallelIterator.html).

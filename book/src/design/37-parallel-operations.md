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

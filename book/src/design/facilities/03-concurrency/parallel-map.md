# Ordered parallel mapping

`pool.map(function, inputs) -> Result[list[U], PoolMapError]` requires a named
one-argument function `(T) -> U`, where U is non-unit and owned storable data.
It consumes an owned `list[T]` or copies an integer `range[T]` normally. Generic
worker types are inferred. Ordinary [worker restrictions](index.md) apply.

The result preserves input order regardless of completion order. Mapping is
eager: output capacity is reserved before execution, and at most
`max_workers + queue_capacity` jobs remain outstanding. Each job cell can fail
allocation. Memory is the output list plus that bounded window, rather than one
future per input.

`PoolMapError` has `Allocation(AllocError)` and `Shutdown`. Infrastructure
failure stops submissions, waits for accepted work, and drops partial outputs
and remaining inputs. The input list is consumed on every outcome; previous
effects remain visible. Allocation failure leaves the pool usable. A closed pool
returns Shutdown, including for empty input. Worker Result values remain ordinary
list elements.

Borrowed inputs, generators, multiple iterables, closure mapping, and streaming
output are unsupported.

## Fallible workers

`pool.map_result(function, inputs)` instead accepts `(T) -> Result[U, E]`
and returns `Result[list[U], ParallelError[E]]`. U must be non-unit; inputs,
outputs, and errors meet owned job eligibility. `ParallelError[E]` has
`Allocation(AllocError)`, `Shutdown`, and `Worker(E)`, with allocation-free error
transport.

Success follows input order. If several workers fail, the lowest input index's
error wins. A failed submission stops further submission; an earlier accepted
input's worker error takes precedence over that submission failure. Allocation
failure before any submission returns Allocation; a closed pool returns Shutdown,
including for empty input.

On observing a worker error, submission stops and all accepted jobs are drained.
Partial successes, later errors, and unsubmitted inputs drop exactly once.
Some later inputs may already have run, and effects are not undone. Workers must
not depend on every input being scheduled. Output capacity and the bounded job
window are reserved before execution; submitted cells remain fallible.

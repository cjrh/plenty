# Ordered parallel mapping

`pool.map(worker, inputs) -> Result[list[U], PoolMapError]` accepts a statically
named function or a concrete reusable immutable closure `(T) -> U`, where U
is non-unit and owned storable data.
It consumes an owned `list[T]` or copies an integer `range[T]` normally. Generic
worker types are inferred. Ordinary [worker restrictions](index.md) apply.

A closure moves into the operation and must wholly own its captures. One
environment is shared by all jobs and remains pinned until every accepted job
finishes, including on empty input, allocation failure, worker error, and
shutdown. It drops once on the owning thread before the map call returns.
Captures are neither copied per input nor given separate heap environments.
Mutable and one-shot closures cannot be shared across jobs; borrowed closures
belong in scoped `spawn`. Concrete closures passed through an owned generic
`F: Callable[[T], U]` parameter remain eligible. Indirect code-address `Callable`
values lack worker-effect evidence and are rejected, even if initialized from a
named function; pass that function directly instead.

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

Borrowed inputs, generators, multiple iterables, and streaming
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

A [parallel pipeline](../../language/09-iteration/06-pipelines.md#parallel-pipelines)
builds on `map` and `map_result`: `iter(items).par(&pool)` maps and filters
contiguous chunks on the pool. Unlike `pool.map`, it also accepts a borrowed
list, `iter(&items)`, whose chunks read the caller's rows in place.

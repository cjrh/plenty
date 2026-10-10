# Executors and shutdown

`ThreadPoolExecutor(max_workers: u64, queue_capacity: u64)` returns
`Result[ThreadPoolExecutor, PoolError]`. Both counts must be positive. Construction
starts all fixed native workers before success and provides a bounded FIFO queue.
Partial startup failure stops and joins workers already started. `PoolError`
has `InvalidSize`, `Allocation(AllocError)`, and `Thread(ThreadError)`.

| Operation | Result | Behavior |
| --- | --- | --- |
| `pool.submit(job)` | `Result[Future[T], SubmitError[F]]` | Move an owned zero-argument job; wait for space |
| `pool.submit(function, arguments...)` | Same, with a bound job type | Bind a statically named function and owned arguments |
| `pool.submit_nowait(...)` | Same | Return `Full(job)` if the queue is full |
| `pool.shutdown()` | `()` | Stop accepting, drain accepted work, join workers |
| `pool.shutdown(True)` | `()` | Skip pending jobs and join running workers |

Methods observe the pool; `mut` is unnecessary. Executors are affine and do not
support copying or equality. Jobs may differ in concrete argument/result types on
one pool. Reusable closures run once then release their environment; one-shot
closures transfer captures into their body. Named submissions infer ordinary
argument types and return a callable bound job on failure.

Inputs and results must be wholly owned and satisfy the [worker checks](index.md).
Borrowed captures/arguments, generator frames, and uncertified indirect effects
are rejected. Executors and futures cannot enter workers. Channel protocols can
still deadlock.

`SubmitError[F]` has `Full(job)`, `Shutdown(job)`, `OutOfMemory(job)`, and
`CapacityOverflow(job)`. Each owns the complete unstarted job for retry,
synchronous invocation, or drop. Blocking submit never returns Full. Shutdown is
reported before job allocation; waiting for space precedes allocation and transfer.
`submit_nowait` can contend on synchronization.

Construction uses one allocation; each accepted job needs one fallible cell.
Status, cancellation, result transfer, waiting, and shutdown allocate nothing.

## Lifetime

`with ThreadPoolExecutor(workers, capacity)? as pool:` owns the executor and
lends `&ThreadPoolExecutor`. Every normal exit drains accepted work and joins
workers; ordinary drop does the same. Explicit shutdown is idempotent and always
waits. Detachment and `wait=False` are unsupported; running jobs are not forcibly
killed. [Tokens](cancellation.md) permit cooperative stopping.

When jobs send to a receiver the parent may abandon, put the receiver guard after
the executor manager so disconnection precedes shutdown. Workers must finish any
other communication before the scope can close. See [futures](futures.md) for
result ownership and [parallel operations](parallel-map.md) for batched work.

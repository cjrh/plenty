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

Workers must finish their communication before the scope can close. See
[futures](futures.md) for result ownership and
[parallel operations](parallel-map.md) for batched work.

## Shutdown and live receivers

Shutdown waits for every accepted job, and a job waiting for queue space is
released only by a receive or by the drop of the last receiver. The compiler
therefore applies the [task join rule](channels.md#joins-and-live-endpoints) to
pools:

> A pool cannot be shut down while a live binding of the function can hold
> `Receiver[T]`, once the function has submitted a job that can hold `Sender[T]`.

A pool shuts down at `pool.shutdown()`, including through a
`&ThreadPoolExecutor`, at `drop(pool)`, when its binding is reassigned, and
wherever its owner leaves scope: the end of its `with` block or of the function,
`return`, `?`, `break`, and `continue`. Locals are released last declared first,
so a receiver bound after the pool is dropped before the pool shuts down.

Jobs are not matched to pools and are never assumed to have finished. A job
submitted with `submit` or `submit_nowait` counts at every later shutdown in the
function, and at every shutdown in a loop that contains the submission. A
`submit(...)?` that fails has submitted nothing on the path that leaves.
`map`, `map_result`, and `reduce_tree` finish their jobs before returning and do
not count. What a job and a binding *can hold* is defined as for tasks.

Place the receiver guard directly after the pool manager. Contexts exit in
reverse order, so every exit drops the receiver before the pool shuts down:

```text
with ThreadPoolExecutor(2, 8)? as pool, receiver as inbox:
    future = pool.submit(produce, sender)?
    ...
```

A guard nested inside the pool's block, a channel created inside it, and a
`drop(receiver)` on every path to the shutdown also satisfy the rule. The
diagnostic reports the shutdown or the pool's binding, each binding that holds a
matching receiver, where the job is submitted, and the early exit that reaches
the shutdown.

The rule has the task rule's
[false positives and gaps](channels.md#what-the-rule-does-not-decide). It also
rejects these safe programs:

- the job has finished, for example after `future.result()` returned;
- the job was submitted to a different pool than the one shutting down;
- a `submit` failure handled without `?` returned the job to the caller.

It does not cover:

- a job submitted by another function through a `&ThreadPoolExecutor`;
- a pool moved into a function that drops it;
- `future.result()` waiting for a job that is waiting to send;
- a `map`, `map_result`, or `reduce_tree` callback that waits to send.

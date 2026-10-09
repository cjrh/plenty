# Executor ownership, errors, and shutdown

## Recoverable errors

`PoolError` has `InvalidSize`, `Allocation(AllocError)`, and `Thread(ThreadError)`
variants. Invalid worker/queue counts, size overflow, allocation exhaustion, and
native thread startup failures are all reported. No partially started executor
escapes a failed constructor.

`SubmitError[F]` has `Full(job)`, `Shutdown(job)`, `OutOfMemory(job)`, and
`CapacityOverflow(job)`. Every variant owns the complete unstarted job. The caller
can retry it, run it synchronously, or let it drop. Blocking `submit` never returns
`Full`. A closed executor reports `Shutdown` before allocating a cell. Waiting
for queue space happens before cell allocation and transfer. `submit_nowait`
does not wait for space but can contend on a mutex.

`FutureError.Cancelled` is the only result-retrieval error. Cancellation succeeds
only before execution begins; repeated cancellation of a cancelled job returns
`True`. It returns `False` for running or completed jobs. Cancellation makes
`done()` true immediately and `result()` returns `Err` without waiting. The queue
still holds its cell until a worker skips it; the captured inputs are dropped
when the queue and public future have both released their ownership. Cancelling
does not forcibly remove a ring entry or immediately guarantee submission space.

`PoolMapError` has `Allocation(AllocError)` and `Shutdown`. Map consumes its input
list even when infrastructure fails. Its submitted prefix completes before the
error returns; unsubmitted inputs and partial outputs are reclaimed. The pool
remains usable after map allocation failure.

All these error wrappers are inline, including a recovered closure environment.
They require no error-object allocation. `Failure` propagation deliberately
discards details and drops any recovered job, as for other error types.

## Scope and handle lifetime

`with ThreadPoolExecutor(workers, capacity)? as pool:` owns the executor and lends
`pool: &ThreadPoolExecutor`. Every normal exit, including `return`, `?`, `break`,
and `continue`, drains accepted jobs and joins the workers. Ordinary executor drop
has the same behavior, so `with` is convenient rather than mandatory. Explicit
`shutdown` is idempotent and always waits; there is no detach or `wait=False` mode.
Default shutdown executes queued work; `shutdown(True)` skips work still queued.
Running jobs are never forcibly killed. Traps/process aborts do not unwind.

Dropping a future does not cancel its job or wait. The queue/worker keeps an
independent owner, finishes the work, then destroys an unclaimed result. A future
that survives executor shutdown retains its completed result until retrieval or
drop. Mutable results transfer exactly once. Future handles never retain the
executor itself, so a retained result cannot prevent pool shutdown.

If workers send to a channel and the parent can stop receiving early, put the
receiver guard after the executor manager. It then disconnects before shutdown
joins workers. Jobs must handle disconnection and terminate. Complete any other
communication needed by workers before leaving their executor scope.

## Runtime and allocation

The current Linux runtime uses the same fallible native thread start/join path
as scoped threads. One allocation contains the executor, pinned worker records,
and bounded pointer ring. Each accepted job allocates one cell with its complete
input environment and result layout. Mutex/condition-variable waits, status,
cancellation, result transfer, and shutdown add no allocations on the supported
target. User code and destructors retain their ordinary allocation effects.

Queue and job-state locks are separate; callbacks and destructors execute outside
both. Inline ranges and environments relocate before their source storage is
reclaimed. Atomic owners preserve cells across the worker/result-consumer race.
There is no strict waiter fairness guarantee, work stealing, or automatic loop
parallelization. Timeouts, public cooperative cancellation tokens, worker-side
submission, and waiting on futures from workers are not implemented.

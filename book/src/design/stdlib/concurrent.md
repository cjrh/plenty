# `std.concurrent`

Explicit imports for [executors](../facilities/03-concurrency/executors.md),
[futures](../facilities/03-concurrency/futures.md),
[parallel map](../facilities/03-concurrency/parallel-map.md), and
[tree reduction](../facilities/03-concurrency/parallel-reduce.md).
Exports: `ThreadPoolExecutor`, `Future[T]`, `PoolError`, `SubmitError[F]`,
`FutureError`, `PoolMapError`, and `ParallelError[E]`. Imports preserve prelude
identity and add no allocation or wrapper. Supported target: x86_64 Linux GNU.

```plenty
from std.concurrent import ThreadPoolExecutor, PoolError, FutureError, SubmitError

def square(n: i64) -> i64:
    n * n

def main() -> Result[(), AllocError | IoError | PoolError | FutureError | SubmitError[()]]:
    with ThreadPoolExecutor(2, 4)? as pool:
        first = pool.submit(square, 7).drop_job()?
        second = pool.submit(square, 8).drop_job()?
        print(first.result()?, second.result()?)?
    Ok(())
```
```output
49 64
```

`ThreadPoolExecutor(workers: u64, queue_capacity: u64)` returns
`Result[ThreadPoolExecutor, PoolError]`; both arguments must be positive.
`PoolError` has `InvalidSize`, `Allocation(AllocError)`, and `Thread(ThreadError)`.
The linked pages specify submission, mapping, reduction and shutdown signatures.
Accepted jobs and results are wholly owned; borrowing belongs in scoped threads.
Pools and futures cannot enter workers. All reachable code still passes worker
effect checking.

Construction makes one allocation and starts the requested OS workers. Each
accepted job needs one fallible cell; a full queue bounds outstanding queued
jobs and can block `submit`. `submit_nowait` reports `SubmitError.Full(job)`;
other variants are `Shutdown(job)`, `OutOfMemory(job)`, and `CapacityOverflow(job)`.
Recovery retains ownership without a hidden clone. `submit(...).drop_job()`
drops a rejected job at the call and reports the job-free `SubmitError[()]`. Waiting, result transfer,
status and cancellation allocate nothing. Queue bookkeeping is constant apart
from job/result transfer and cleanup; waiting and shutdown have no bounded
latency. Map allocates outputs and uses a bounded work window; reduction costs
and ordering are specified in its facility page.

`Future[T].result()` consumes the future and returns `Result[T, FutureError]`.
Cancellation before execution yields `FutureError.Cancelled`; it never forcibly
stops running code. `done()`, `cancel()`, and `wait_timeout(timeout_ms)` observe
the same state as the prelude methods. Use [`std.sync`](sync.md) tokens for
cooperative stopping. Pool exit drains and joins; dropping a future does not
cancel its job. Live-receiver shutdown and result-wait diagnostics also apply
through imports. There is no second future or cancellation model.

## Acknowledgements

- `ThreadPoolExecutor` is modelled on [Python's `concurrent.futures`](https://docs.python.org/3/library/concurrent.futures.html).

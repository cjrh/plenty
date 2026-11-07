# Reuse workers with an executor

A thread pool reuses workers for many jobs. Set its worker count and queue
capacity at construction:

```plenty
import std.system

def square(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * value)

type PoolJobError = ParallelError[OverflowError[i64]] | FutureError | SubmitError[()] | PoolError

def main() -> Result[(), IoError | PoolJobError | OverflowError[i64]]:
    workers = std.system.available_parallelism().unwrap_or(1)
    with ThreadPoolExecutor(workers, 4)? as pool:
        first = pool.submit(square, 6).drop_job()?
        second = pool.submit(square, 9).drop_job()?
        print(first.result()??)?
        print(second.result()??)?
        print(pool.map_result(square, range(6))?)?
    Ok(())
```

```output
36
81
[0, 1, 4, 9, 16, 25]
```

`available_parallelism()` estimates usable CPU parallelism; this example chooses
one worker if the query fails. Query it once when creating the pool, and preserve
a user-supplied worker count if your application has one. The estimate is useful
for CPU-bound work; I/O-heavy jobs may need a different measured count. Linux
uses affinity and visible conventional cgroup quotas; platform approximations
and failures are explained in [`std.system`](../../design/stdlib/system.md#available-parallelism).

`submit` returns an owned `Future[Result[i64, OverflowError[i64]]]`;
`result()` waits and consumes it. The first `?` handles the future wait,
and the second propagates the worker's overflow error.
A failed submission returns `SubmitError`, which owns the unstarted job.
`drop_job()` drops that job, as for [thread starts](owned-worker-jobs.md), and
leaves `SubmitError[()]`, which still says why submission failed.
Submit jobs before waiting for their results to let them overlap. Leaving `with`
waits for accepted jobs and joins every worker, including on early return or
propagated error.

`map_result` returns successful values in input order, or the first worker
error in input order. `map` instead keeps every returned value, including
any `Result` wrappers. It accepts an owned list or an integer range
and a named function or owned reusable immutable closure, keeping a bounded
window of jobs outstanding. Job and
result storage can fail to allocate.

```plenty
def main() -> Result[(), AllocError | IoError | ParallelError[OverflowError[i64]] | PoolError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        factor = 3
        scale = def [factor](value: i64) -> Result[i64, OverflowError[i64]]:
            Ok(value * factor)
        print(pool.map_result(scale, range(4))?)?
    Ok(())
```

```output
[0, 3, 6, 9]
```

Mapping consumes `scale` and shares its one immutable environment until all jobs
finish. Mutable, one-shot, or borrowed closures cannot be shared this way.
`map_result` accepts the same workers when they return `Result[T, E]`.

You can submit a closure to move several values into a job explicitly:

```plenty
type PoolJobError = FutureError | SubmitError[()] | PoolError

def main() -> Result[(), AllocError | IoError | PoolJobError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        values = [3, 6, 9]?
        future = pool.submit(def once [values]() -> list[i64]: values).drop_job()?
        print(future.result()?)?
    Ok(())
```

```output
[3, 6, 9]
```

Pool jobs own their inputs; use `spawn` when a worker needs to borrow local data.
A worker may use channels, following the [same habits](wait-habits.md) as a
task, but cannot hold an executor or future.
Keep submission and future waits on the owning thread to avoid pool dependency
deadlocks. Files, native effects and foreign calls follow the same rules as for scoped workers.

Dropping a future leaves its job running; an unclaimed result is cleaned up.
A future may also outlive the pool's scope: shutdown finishes the job, and the
handle retains the result until you retrieve or drop it.

Keep the job instead when you want to retry it or run it locally; a
[submission backpressure lesson](submission-backpressure.md) shows how.

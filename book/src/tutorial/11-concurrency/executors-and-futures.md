# Reuse workers with an executor

A thread pool reuses workers for many jobs. Set its worker count and queue
capacity at construction:

```plenty
def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(SubmitError.Full(_)):
            Err("submission queue is full")
        case Err(SubmitError.Shutdown(_)):
            Err("executor has shut down")
        case Err(SubmitError.OutOfMemory(_)):
            Err("submission allocation failed")
        case Err(SubmitError.CapacityOverflow(_)):
            Err("submission capacity overflow")

def square(value: i64) -> i64:
    value * value

type PoolJobError = PoolMapError | FutureError | str | PoolError

def main() -> Result[(), IoError | PoolJobError]:
    with ThreadPoolExecutor(2, 4)? as pool:
        first = submitted(pool.submit(square, 6))?
        second = submitted(pool.submit(square, 9))?
        print(first.result()?)?
        print(second.result()?)?
        print(pool.map(square, range(6))?)?
    Ok(())
```

```output
36
81
[0, 1, 4, 9, 16, 25]
```

`submit` returns an owned `Future[i64]`; `result()` waits and consumes it.
Submit jobs before waiting for their results to let them overlap. Leaving `with`
waits for accepted jobs and joins every worker, including on early return or
propagated error.

`map` returns a list in input order. It accepts an owned list or an integer range
and a named function or owned reusable immutable closure, keeping a bounded
window of jobs outstanding. Job and
result storage can fail to allocate.

```plenty
def main() -> Result[(), AllocError | IoError | PoolMapError | PoolError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        factor = 3
        scale = def [factor](value: i64) -> i64:
            value * factor
        print(pool.map(scale, range(4))?)?
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
def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(SubmitError.Full(_)):
            Err("submission queue is full")
        case Err(SubmitError.Shutdown(_)):
            Err("executor has shut down")
        case Err(SubmitError.OutOfMemory(_)):
            Err("submission allocation failed")
        case Err(SubmitError.CapacityOverflow(_)):
            Err("submission capacity overflow")

type PoolJobError = FutureError | str | PoolError

def main() -> Result[(), AllocError | IoError | PoolJobError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        values = [3, 6, 9]?
        future = submitted(pool.submit(def once [values]() -> list[i64]: values))?
        print(future.result()?)?
    Ok(())
```

```output
[3, 6, 9]
```

Pool jobs own their inputs; use `spawn` when a worker needs to borrow local data.
A worker may use channels, but cannot hold an executor or future.
Keep submission and future waits on the owning thread to avoid pool dependency
deadlocks. Files, native effects and foreign calls follow the same rules as for scoped workers.

Dropping a future leaves its job running; an unclaimed result is cleaned up.
A future may also outlive the pool's scope: shutdown finishes the job, and the
handle retains the result until you retrieve or drop it.

Submission errors retain an unstarted job. Here `submitted` deliberately
releases that job and preserves the failure reason as a message. Match the
original `SubmitError` instead when you want to recover or retry the job.

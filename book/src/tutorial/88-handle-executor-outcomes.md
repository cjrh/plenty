# Handle executor outcomes

A job returning `Result` keeps its application errors separate from submission
failure or cancellation. Retrieve its result, then handle the application result:

```plenty
def divide(value: i64) -> Result[i64, str]:
    if value == 0:
        return Err("zero divisor")
    Ok(24 // value)

def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(2, 2)? as pool:
        future = pool.submit(divide, 0)?
        match future.result()?:
            case Ok(value):
                print(value)?
            case Err(message):
                print(message)?
        print(pool.map(divide, [2, 0, 4]?)?)?
    Ok(())
```

```output
zero divisor
[Result[i64, str].Ok(12), Result[i64, str].Err("zero divisor"), Result[i64, str].Ok(6)]
```

Mapping preserves each application outcome in input order. To propagate both
layers from a single future, write `(future.result()?)?`.

`submit` waits for queue space. `submit_nowait` instead returns the unstarted job
in `SubmitError.Full(job)` when full. Every submission error owns the job, so a
generic helper can run it locally when submission is unavailable:

```plenty
def run_or_submit[F: OnceCallable[[], i64]](
    pool: &ThreadPoolExecutor, job: F
) -> Result[i64, FutureError]:
    match pool.submit_nowait(job):
        case Ok(future):
            future.result()
        case Err(error):
            match error:
                case SubmitError[F].Full(job):
                    Ok(job())
                case SubmitError[F].Shutdown(job):
                    Ok(job())
                case SubmitError[F].OutOfMemory(job):
                    Ok(job())
                case SubmitError[F].CapacityOverflow(job):
                    Ok(job())

def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(1, 1)? as pool:
        pool.shutdown()
        number = 42
        job = def once [number]() -> i64:
            number
        print(run_or_submit(pool, job)?)?
    Ok(())
```

```output
42
```

Here the executor is already shut down, so the helper takes the fallback path.
The recovered job needs no allocation. The ordinary `?` alternative deliberately
drops that job when propagating its error into `Failure`.

`future.cancel()` returns `True` when it prevents a queued job from starting.
It cannot interrupt running work. A cancelled future reports
`FutureError.Cancelled` from `result()`. `future.done()` observes completion or
cancellation without consuming the handle. Explicit `pool.shutdown(True)` cancels
jobs still queued and waits for running jobs; ordinary scope exit drains all work.
If a job communicates through channels, finish or disconnect that communication
before waiting for shutdown, as in the channel lessons.

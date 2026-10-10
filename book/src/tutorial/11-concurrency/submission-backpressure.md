# Recover a job when submission cannot proceed

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

Propagating a submission error into `Failure` with `?` instead drops the
unstarted job.

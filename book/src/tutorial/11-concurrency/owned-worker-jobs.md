# Move jobs to workers

An owned closure can move its inputs into a worker. Use a one-shot closure when
the worker also moves those inputs into its result.

```plenty
def main() -> Result[(), AllocError | IoError | SpawnError[OnceClosure[[], list[i64]]]]:
    values = [3, 4]?
    with spawn(def once [values]() -> list[i64]: values)? as task:
        print(task.join())?
    Ok(())
```

```output
[3, 4]
```

If thread creation fails, the error is `SpawnError[F].Unavailable(job)`, which
retains the complete job. `F` is the closure's own type. The signature writes
`OnceClosure[[], list[i64]]`, and `?` fills in the concrete closure it
propagates, so `main` returns the unstarted job unchanged.

A signature names one closure there: a second `spawn` of a different closure
literal is rejected. Make each job with one factory function, or use a generic
helper, which names the job as `F`. Recover the job to retry or run it
locally, as this helper does:

```plenty
def attempt[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())

def run[F: OnceCallable[[], i64]](job: F) -> i64:
    match attempt(job):
        case Ok(value):
            value
        case Err(SpawnError.Unavailable(job)):
            job()

def main() -> Result[(), IoError]:
    n = 42
    job = def once [n]() -> i64:
        n
    print(run(job))?
    Ok(())
```

```output
42
```

A reusable closure can also be moved into `spawn`; its captures are dropped
after the worker's single invocation.

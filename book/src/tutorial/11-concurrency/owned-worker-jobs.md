# Move jobs to workers

An owned closure can move its inputs into a worker. Use a one-shot closure when
the worker also moves those inputs into its result.

```plenty
def main() -> Result[(), Failure]:
    values = [3, 4]?
    job = def once [values]() -> list[i64]:
        values
    with spawn(job)? as task:
        print(task.join())?
    Ok(())
```

```output
[3, 4]
```

`values` moves into `job`, then `job` moves into the worker. Neither binding can
be used afterward. Creation failure retains the complete job in its error. This
generic helper lets callers recover it; here a failed start falls back to running
the same job on the current thread.

```plenty
def attempt[F: OnceCallable[[], i64]](job: F) -> Result[i64, SpawnError[F]]:
    with spawn(job)? as task:
        return Ok(task.join())

def run[F: OnceCallable[[], i64]](job: F) -> i64:
    match attempt(job):
        case Ok(value):
            value
        case Err(error):
            match error:
                case SpawnError[F].Unavailable(job):
                    job()
                case SpawnError[F].PermissionDenied(job):
                    job()

def main() -> Result[(), Failure]:
    n = 42
    job = def once [n]() -> i64:
        n
    print(run(job))?
    Ok(())
```

```output
42
```

The error wrapper and closure environment need no allocation. Using `?` in a
function returning `Failure` intentionally discards that error and drops the
unstarted job. A reusable closure can also be moved into `spawn`; its captures
are dropped after the worker's single invocation.

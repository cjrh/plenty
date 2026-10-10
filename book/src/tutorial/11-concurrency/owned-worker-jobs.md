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

If thread creation fails, its error retains the complete job. Recover it to
retry or run it locally, as this helper does:

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

Using `?` in a function returning `Failure` discards the creation error and drops
the unstarted job. A reusable closure can also be moved into `spawn`; its captures
are dropped after the worker's single invocation.

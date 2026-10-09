# Reuse workers with an executor

A thread pool reuses a fixed number of workers for many jobs. Give it both a
worker count and a queue capacity; these limits make resource use explicit.

```plenty
def square(value: i64) -> i64:
    value * value

def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(2, 4)? as pool:
        first = pool.submit(square, 6)?
        second = pool.submit(square, 9)?
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

`submit` returns a `Future[i64]`: an owned handle for one eventual result.
`result()` waits and consumes that handle. Both submissions happen before the
first wait, so the jobs can overlap. Their completion order does not affect this
example's print order. Leaving `with` waits for accepted jobs and joins every
worker, even on an early `return` or propagated error.

`map` returns a list in input order. It accepts an owned list or an integer range
and a named function, with generic types inferred normally. It keeps only a
bounded window of jobs outstanding. The result list and each job cell can fail
to allocate, so the operation returns a `Result`.

You can submit a closure to move several values into a job explicitly:

```plenty
def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(2, 2)? as pool:
        values = [3, 6, 9]?
        job = def once [values]() -> list[i64]:
            values
        future = pool.submit(job)?
        print(future.result()?)?
    Ok(())
```

```output
[3, 6, 9]
```

The list moves into the job and back through its result. No implicit list copy
occurs. Pool jobs own their inputs; use `spawn` when a worker needs to borrow
local data. A worker may use channels, but cannot hold an executor or future.
Keep submission and future waits on the owning thread to avoid pool dependency
deadlocks. Files and foreign effects have the same restrictions as scoped workers.

Dropping a future leaves its job running; an unclaimed result is cleaned up.
A future may also outlive the pool's scope: shutdown finishes the job, and the
handle retains the result until you retrieve or drop it.

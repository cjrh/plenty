# Reuse workers with an executor

A thread pool reuses workers for many jobs. Set its worker count and queue
capacity at construction:

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

`submit` returns an owned `Future[i64]`; `result()` waits and consumes it.
Submit jobs before waiting for their results to let them overlap. Leaving `with`
waits for accepted jobs and joins every worker, including on early return or
propagated error.

`map` returns a list in input order. It accepts an owned list or an integer range
and a named function, keeping a bounded window of jobs outstanding. Job and
result storage can fail to allocate.

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

Pool jobs own their inputs; use `spawn` when a worker needs to borrow local data.
A worker may use channels, but cannot hold an executor or future.
Keep submission and future waits on the owning thread to avoid pool dependency
deadlocks. Files and foreign effects have the same restrictions as scoped workers.

Dropping a future leaves its job running; an unclaimed result is cleaned up.
A future may also outlive the pool's scope: shutdown finishes the job, and the
handle retains the result until you retrieve or drop it.

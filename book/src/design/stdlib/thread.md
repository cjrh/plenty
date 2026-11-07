# `std.thread`

Explicit imports for [scoped threads](../facilities/03-concurrency/threads.md).
Exports: `spawn`, `ThreadError`, and `SpawnError[F]`. These are the same compiler
facilities and types available in the prelude, including through module and
item aliases. Importing allocates nothing and introduces no wrapper or new
execution model. Supported target: x86_64 Linux GNU.

```plenty
from std.thread import spawn, SpawnError

def total(values: &list[i64], start: i64, stop: i64) -> Result[i64, OverflowError[i64]]:
    mut sum = 0
    for i in range(start, stop):
        sum += values[i]
    Ok(sum)

def main() -> Result[(), AllocError | IoError | SpawnError[()] | OverflowError[i64]]:
    values = [n for n in range(1, 101)]?
    with spawn(total, &values, 0, 50)? as low, spawn(total, &values, 50, 100)? as high:
        print(low.join()? + high.join()?)?
    Ok(())
```
```output
5050
```

Use `with spawn(worker, arguments...)? as task:` for a named function, or
`with spawn(job)? as task:` for an owned zero-argument job. `task.join()` waits
and moves the result once; leaving the scope joins automatically. Tasks remain
lexical handles and cannot escape. Borrowed arguments retain their loans until
join. The ordinary worker eligibility, foreign-effect, and channel hang checks
apply to every imported spelling.

A start with only owned or copied arguments, `spawn(job)` or
`spawn(worker, arguments...)`, reports `SpawnError[F].Unavailable(job, code)`,
retaining the job for recovery; a named start's job binds its arguments.
`.drop_job()` on the start drops the job and reports the job-free
`SpawnError[()]`. A start that borrows, such as `spawn(total, &values, 0, 50)`,
reports `SpawnError[()].Unavailable((), code)` and drops its moved arguments,
since a loan cannot be returned in an error. `ThreadError` remains the payload
of `PoolError.Thread`. See
[owned jobs](../facilities/03-concurrency/jobs.md) and
[start failures](../facilities/03-concurrency/threads.md#start-failures). Job handoff and bookkeeping
allocate no Plenty heap storage; native thread creation uses OS resources.
There is no hidden clone. Start/join bookkeeping has fixed overhead apart from
argument/result transfer and cleanup, but scheduling and joining have no bounded
latency. A join waits as long as the worker needs.

Scoped `spawn(...)?` propagates the start error like any `?`: the enclosing
Result's error must be the start error, or an anonymous sum containing it.
`spawn(...).unwrap()` traps on a failed start.

The prelude spellings remain supported. There is no detached thread, global
pool, forced interruption, or import-only exemption from ownership checks.
For examples see [explicit concurrency imports](../../tutorial/11-concurrency/explicit-imports.md).

To size a worker pool, use [`std.system.available_parallelism()`](system.md#available-parallelism),
which returns a fallible positive estimate with caller-selected fallback.

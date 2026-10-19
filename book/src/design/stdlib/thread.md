# `std.thread`

Explicit imports for [scoped threads](../facilities/03-concurrency/threads.md).
Exports: `spawn`, `ThreadError`, and `SpawnError[F]`. These are the same compiler
facilities and types available in the prelude, including through module and
item aliases. Importing allocates nothing and introduces no wrapper or new
execution model. Supported target: x86_64 Linux GNU.

Use `with spawn(worker, arguments...)? as task:` for a named function, or
`with spawn(job)? as task:` for an owned zero-argument job. `task.join()` waits
and moves the result once; leaving the scope joins automatically. Tasks remain
lexical handles and cannot escape. Borrowed arguments retain their loans until
join. The ordinary worker eligibility, foreign-effect, and channel hang checks
apply to every imported spelling.

Named/borrowed starts report `ThreadError.System(code)`. An owned start reports
`SpawnError[F].Unavailable(job)`, retaining the job for recovery; see
[owned jobs](../facilities/03-concurrency/jobs.md). Job handoff and bookkeeping
allocate no Plenty heap storage; native thread creation uses OS resources.
There is no hidden clone. Start/join bookkeeping has fixed overhead apart from
argument/result transfer and cleanup, but scheduling and joining have no bounded
latency. A join waits as long as the worker needs.

Scoped `spawn(...)?` propagates the start error like any `?`: the enclosing
Result's error must be the start error, an anonymous sum containing it, or
`Failure`. `spawn(...).unwrap()` traps on a failed start.

The prelude spellings remain supported. There is no detached thread, global
pool, forced interruption, or import-only exemption from ownership checks.
For examples see [explicit concurrency imports](../../tutorial/11-concurrency/explicit-imports.md).

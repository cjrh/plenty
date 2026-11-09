# Scoped threads

`with spawn(worker, arguments...)? as task:` starts a native thread and joins it
before leaving the block. Named functions, including inferred generic instances,
use their ordinary typed parameters. `spawn(&job)` or `spawn(&mut job)` runs an
explicitly borrowed reusable zero-argument closure. A binding that already holds
a `&Closure` or `&mut Closure`, such as a parameter or a `get_mut` match binding,
is reborrowed for the task's scope: `spawn(callback)` means `spawn(&*callback)`,
or `spawn(&mut *callback)` when the closure has mutable captures, which requires a
`&mut` reference. Wholly owned jobs use [consuming spawn](jobs.md).

`task.join()` waits and transfers the result exactly once, consuming the task
binding. A worker returning `Result[T, E]` yields that same result. Join itself
is infallible under the compiler/runtime contract; unexpected native join failure
terminates the process to avoid reclaiming storage still used by a worker.

Omitting `as`, or leaving a task unjoined, waits and drops its result at exit.
Application errors disappear with an unclaimed result; join explicitly to inspect
or propagate them. Owned results, including collections, recursive records, and
owned concrete callbacks, can transfer back. References, borrowed environments,
and generator frames cannot be returned from a worker.

## Scope and loans

Task bindings are lexical join tokens: they cannot be copied, borrowed, returned,
stored, captured, or passed to a function. `spawn` must be the direct manager
expression, using `?` or `.unwrap()`. Comma-separated managers form nested scopes.
Each successful task joins on normal completion, `return`, `?`, `break`, and
`continue`. Failure to start a later task first joins earlier tasks. Detachment
and forced cancellation are unsupported. A join that could wait for a task
holding a token before the function's later cancel is rejected; put
`stop.cancel_on_exit()` after the task to cancel it on every exit. See
[cancel on every exit](cancellation.md#cancel-on-every-exit).

A join, explicit or automatic, is rejected while the function keeps a channel
endpoint that the worker could be waiting on. See
[joins and live endpoints](channels.md#joins-and-live-endpoints) for the rule
and its [recommended patterns](channels.md#recommended-patterns).

Argument loans and borrowed capture dependencies last until the end of the
`with` block, including after explicit join. Shared readers can coexist;
exclusive loans prohibit conflicting access, moves, or drops. Ordinary field
disjointness and enum-payload rules apply. `yield` inside a task scope is rejected.

Scoped work may borrow or own its inputs. Named arguments use ordinary call
semantics: `&T` and `&mut T` borrow, copyable values copy, and affine values move
into the worker. A generator argument, owned or borrowed, is rejected with a
diagnostic naming the parameter, because generator frames cannot enter workers;
collect its items into a list and pass that. A
[lent pipeline](../../language/09-iteration/06-pipelines.md#lending-a-pipeline),
`spawn(worker, &mut p)` with `worker` taking `&mut Iter[T]`, runs in the worker
and writes its state back when the worker returns; the lender continues after
the items the worker took once the `with` block ends. Callbacks the worker
runs pass the worker checks. Pool jobs must own their inputs instead; only
[`.par` chunks](../../language/09-iteration/06-pipelines.md#parallel-pipelines)
read a borrowed list internally, because the terminal waits for every chunk
before it returns. Borrowing callbacks
from a collection protects the collection through scope exit; separate tuple
slots can have disjoint loans. All workers satisfy the
[shared eligibility rules](index.md).

## Start failures

A named start whose arguments are all owned or copied, or that has none, binds them into a
one-shot job, as [`pool.submit(function, arguments...)`](executors.md) does,
and starts that job. Its start expression has result
`Result[(), SpawnError[F]]`, where `F` is the bound
`OnceClosure[[], R]`; a failed start returns the job with every argument, as an
[owned job start](jobs.md) does. `spawn(worker, sender)` with an owned `Sender`
therefore returns the sender on failure. Call `drop_job()` on the start to drop
the arguments there instead.

A start that borrows, a named start with a `&T` or `&mut T` argument or
`spawn(&job)`, returns the job-free `SpawnError[()].Unavailable((), code)`, so
every start fails with `SpawnError`. A borrowed argument cannot be returned in
an error, so moved arguments of a borrowed start drop exactly once on the parent
thread, in reverse argument order; on success they belong to the worker. The
worker has not run and cannot have changed borrowed inputs; already evaluated
argument expressions keep their effects.

`spawn(...)?` follows ordinary
[`?` propagation](../../language/08-errors/01-sums-and-propagation.md): the
enclosing Result's error may be the start error or an anonymous sum containing
it.

Every nonzero `pthread_create` status is a start failure; none terminates the
process. The error carries it: `SpawnError[F].Unavailable(job, code)`, with
`code: i32`. The usual code is `EAGAIN` (11),
for a thread or memory limit. The runtime passes no thread attributes, so other
codes come only from the environment: a seccomp filter that denies `clone`
surfaces its own code, such as `EPERM` (1).

Task bookkeeping and error handling allocate no Plenty heap storage; native thread
stacks and OS bookkeeping still require resources. The current runtime supports
joinable native threads on x86_64 Linux GNU.

Joining synchronizes worker completion and its cleanup, but does not synchronize
Linux's later removal of the task from `/proc/self/task`. Diagnostic tools can
briefly observe an exiting task there after a successful join; this does not
mean the worker is still running Plenty code or still owns its dropped handles.

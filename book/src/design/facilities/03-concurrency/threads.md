# Scoped threads

`with spawn(worker, arguments...)? as task:` starts a native thread and joins it
before leaving the block. Named functions, including inferred generic instances,
use their ordinary typed parameters. `spawn(&job)` or `spawn(&mut job)` runs an
explicitly borrowed reusable zero-argument closure. Wholly owned jobs use
[consuming spawn](jobs.md).

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
and forced cancellation are unsupported.

A join is rejected when the worker can hold `Sender[T]` and a live binding of
the joining function can hold `Receiver[T]`: a full queue could block the
worker's send and the join forever. Likewise, a worker holding `Receiver[T]`
cannot be joined while the function retains `Sender[T]`: an empty queue could keep
its receive waiting. This applies to `task.join()` and to every automatic join.
See [joins and live endpoints](channels.md#joins-and-live-endpoints).

Argument loans and borrowed capture dependencies last until the end of the
`with` block, including after explicit join. Shared readers can coexist;
exclusive loans prohibit conflicting access, moves, or drops. Ordinary field
disjointness and enum-payload rules apply. `yield` inside a task scope is rejected.

Scoped work may borrow or own its inputs. Named arguments use ordinary call
semantics: `&T` and `&mut T` borrow, copyable values copy, and affine values move
into the worker. Pool work must own its inputs instead. Borrowing callbacks
from a collection protects the collection through scope exit; separate tuple
slots can have disjoint loans. All workers satisfy the
[shared eligibility rules](index.md).

## Start failures

Borrowed and named starts return `ThreadError.System(code)` with the native
start error code. The worker has not run and cannot have changed borrowed inputs;
already evaluated argument expressions keep their effects. `spawn(...)?` follows
ordinary [`?` propagation](../../language/08-errors/01-sums-and-propagation.md):
the enclosing Result's error may be the start error or an anonymous sum containing
it.

Every nonzero `pthread_create` status is a start failure; none terminates the
process. The usual code is `EAGAIN` (11), for a thread or memory limit. The
runtime passes no thread attributes, so other codes come only from the
environment: a seccomp filter that denies `clone` surfaces its own code, such as
`EPERM` (1). [Consuming starts](jobs.md) report all of them as
`SpawnError[F].Unavailable(job)`.

Moved named arguments are consumed even if startup fails: their owners drop
exactly once on the parent thread, in reverse argument order. On success they
belong to the worker. Use an [owned closure job](jobs.md) when startup failure
must return all inputs together for retry; its `SpawnError[F]` retains the job.

Task bookkeeping and error handling allocate no Plenty heap storage; native thread
stacks and OS bookkeeping still require resources. The current runtime supports
joinable native threads on x86_64 Linux GNU.

Joining synchronizes worker completion and its cleanup, but does not synchronize
Linux's later removal of the task from `/proc/self/task`. Diagnostic tools can
briefly observe an exiting task there after a successful join; this does not
mean the worker is still running Plenty code or still owns its dropped handles.

# Scoped native threads

`with spawn(worker, arguments...)? as task:` starts a native thread and joins it
before leaving the block. Named functions, including inferred generic instances,
use their ordinary typed parameters. `spawn(&job)` or `spawn(&mut job)` runs an
explicitly borrowed, reusable zero-argument closure. No new closure annotation is
needed. The body and worker can execute in parallel, without an interpreter lock.

`task.join()` waits and transfers the worker's result to the caller exactly once.
It consumes the task binding; another join, including a possibly repeated join
after a branch, is rejected. A worker returning `Result[T, E]` yields that same
Result. Joining itself is infallible under the compiler/runtime contract.
Omitting `as`, or leaving a task unjoined, waits and drops its result at exit.
Unclaimed application errors are discarded with that result; use `join()` to
inspect or propagate them.

Task bindings are lexical join tokens. They cannot be copied, borrowed, returned,
stored, captured, or passed to another function. `spawn` is currently valid only
as the direct manager expression with `?` or `.unwrap()`. Comma-separated managers
provide nested task scopes. Every successfully started task is joined on normal
completion, `return`, `?`, `break`, and `continue`. If a later start fails, earlier
tasks are joined. There is no detachment, forced cancellation, or unwinding on
fatal traps. A worker that never finishes prevents its scope from finishing.

## Ownership and eligibility

Mutable owners enter through `&T` or `&mut T`, or through the captures of a borrowed
closure. Copyable immutable arguments use ordinary assignment semantics. By-value
affine arguments and consuming closure jobs are rejected: this preserves the
caller's unique job on failed creation. Owned results, including collections and
recursive records, can transfer back at join. Worker results cannot contain
references or inline generator/closure frames.

Argument loans and closure-capture dependencies last through the end of the
`with` block, including after an explicit join. Shared readers may coexist;
exclusive borrows prevent conflicting reads, writes, moves, or drops. Existing
field disjointness and enum-payload loan rules apply. Borrowed storage cannot be
reclaimed while a worker can still access it. `yield` inside the block is rejected.

Eligibility is a closed-world compiler check, not a user-defined marker protocol.
The checker visits finite nominal type graphs and reachable function bodies,
including helper calls, concrete closure calls, and class destructors. Recursive
graphs terminate through visited identities. Scalars, immutable strings, ranges,
ordinary collections, classes, and enums are eligible when their contents and
effects are eligible. Shared storage uses the runtime's atomic reference counts;
those counts do not permit concurrent mutation.

Files, foreign pointers/calls, indirect callable effects, and generator frames
are conservatively excluded from worker code. A capture-free helper or a class
with scalar fields can therefore still be rejected because of its effects or
destructor. Imports do not grant thread permission. Checked ordinary stream I/O
remains usable, with scheduler-dependent ordering between workers.

## Storage and failures

Task arguments and results occupy pinned storage in the parent's native stack
frame. A generated C entry adapter calls the typed Plenty worker and publishes
its result through that storage. The parent cannot read it before joining.
Task bookkeeping and error construction require no Plenty heap allocation.
Native thread stacks and OS bookkeeping still consume resources.

Creation failure propagates an inline `ThreadError.System(code)`, retaining the
native error code. The worker has not run and borrowed inputs remain unchanged by
it; argument expressions already evaluated may have their own effects. Functions
may propagate the error through `Result[..., ThreadError]` or erase it explicitly
into `Failure`. A helper returning the typed error lets callers match a failed
start and retry their retained job.

The supported x86_64 Linux GNU runtime uses joinable POSIX threads. Creation
returns a native error code; joining waits for completion and releases the
thread's resources. See the Linux manual pages for
[creation](https://man7.org/linux/man-pages/man3/pthread_create.3.html) and
[joining](https://man7.org/linux/man-pages/man3/pthread_join.3.html).
An unexpected native join failure terminates the process: continuing could
reclaim storage still used by a worker.

`tests/test_threads.rs` covers source loans, result transfer, nested workers,
generic and recursive types, effect rejection, all normal exits, failed starts,
unclaimed results, and allocation-disabled bookkeeping. Runtime tests require
two workers to overlap and check publication at join.

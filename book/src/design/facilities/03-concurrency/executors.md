# Executors and shutdown

`ThreadPoolExecutor(max_workers: Positive[i64], queue_capacity: Positive[i64])` returns
`Result[ThreadPoolExecutor, PoolError]`. Both counts are `Positive[i64]`; positive literals convert at compile time,
and computed counts use `Positive.try_from(count)?`. Construction
starts all fixed native workers before success and provides a bounded FIFO queue.
Partial startup failure stops and joins workers already started. `PoolError`
has `Allocation(AllocError)` and `Thread(ThreadError)`.

| Operation | Result | Behavior |
| --- | --- | --- |
| `pool.submit(job)` | `Result[Future[T], SubmitError[F]]` | Move an owned zero-argument job; wait for space |
| `pool.submit(function, arguments...)` | Same, with a bound job type | Bind a statically named function and owned arguments |
| `pool.submit_nowait(...)` | Same | Return `Full(job)` if the queue is full |
| `pool.cancel_pending()` | `()` | Cancel every queued job; running jobs continue and the pool stays open |
| `pool.submitter()` | `Submitter` | A handle that workers can use to submit; see [submit from workers](#submit-from-workers) |
| `pool.shutdown()` | `()` | Stop accepting, drain accepted work, join workers |

`cancel_pending()` does not wait for running jobs. It removes the queued jobs,
so their queue slots are free for later submissions; their futures report
`FutureError.Cancelled`, and each job's captures are dropped with its future.
To stop without running queued work, call `pool.cancel_pending()` and then
`pool.shutdown()`. `shutdown()` takes no argument; `shutdown(True)` is rejected
with a diagnostic naming `cancel_pending()`.

Methods observe the pool; `mut` is unnecessary. Executors are affine and do not
support copying or equality. Jobs may differ in concrete argument/result types on
one pool. Reusable closures run once then release their environment; one-shot
closures transfer captures into their body. Named submissions infer ordinary
argument types and return a callable bound job on failure.

Inputs and results must be wholly owned and satisfy the [worker checks](index.md).
Jobs share state through [`Shared[T]`](../01-builtins/shared.md) handles:
pass each job its own `handle.share()`.
Borrowed captures/arguments, generator frames, and uncertified indirect effects
are rejected. Executors and futures cannot enter workers; a
[`Submitter`](#submit-from-workers) can.

User jobs must own their inputs because a future can outlive the borrow that
would feed it. [Parallel pipelines](../../language/09-iteration/06-pipelines.md#parallel-pipelines)
are the one internal exception: `.par` chunk jobs read a list's rows in place,
which is sound because every `.par` terminal waits for all submitted jobs before
returning, including after a failed submission. User-facing scoped pool jobs, a
deferred B22 extension, would build on the same guarantee. Channel protocols can
still deadlock.

`SubmitError[F]` has `Full(job)`, `Shutdown(job)`, `OutOfMemory(job)`, and
`CapacityOverflow(job)`. Each owns the complete unstarted job for retry,
synchronous invocation, or drop. A named submission's job is a one-shot closure
binding the arguments, so `pool.submit(stopped, token)?` can propagate as
`SubmitError[OnceClosure[[], bool]]`; a signature names jobs as for
[owned starts](jobs.md). `pool.submit(job).drop_job()?` instead drops the job
of a rejected submission at the call and gives `SubmitError[()]` with the same
variant, such as `Shutdown(())`; one such member covers every job, as for
[job-free start errors](jobs.md#job-free-errors). Blocking submit never returns Full. Shutdown is
reported before job allocation; waiting for space precedes allocation and transfer.
`submit_nowait` can contend on synchronization.

Construction uses one allocation; each accepted job needs one fallible cell.
Status, cancellation, result transfer, waiting, and shutdown allocate nothing.

## Submission and completion capacity

A scheduler that alone receives its jobs' completions can deadlock in blocking
`submit` while it owes receptions: workers blocked sending to a full completion
channel stop draining the queue, and the scheduler waits for queue space
forever. With one worker, a queue of one and a channel of one, four blocking
submissions before any receive hang. Jobs that return their results through
futures avoid the cycle; [`std.sync.Completions`](../../stdlib/sync.md#completionst)
collects them with a fixed bound on accepted work. With a completion channel,
use `submit_nowait`; on
`Full(job)`, while accepted jobs remain unreceived, receive one completion and
retry the retained job. Accepted-but-unreceived work is then bounded by workers
plus queue capacity plus channel capacity. This assumes one completion per
accepted job on that channel, no other submitters whose jobs wait on the
scheduler, and workers that reach their send; `cancel_pending()` removes jobs
that will never complete. The
[several-jobs lesson](../../../tutorial/11-concurrency/scheduler-window.md)
shows the loop. The compiler does not check capacities or this protocol.

## Lifetime

`with ThreadPoolExecutor(workers, capacity)? as pool:` owns the executor and
lends `&ThreadPoolExecutor`. Every normal exit drains accepted work and joins
workers; ordinary drop does the same. Explicit shutdown is idempotent and always
waits. Detachment and `wait=False` are unsupported; running jobs are not forcibly
killed. [Tokens](cancellation.md) permit cooperative stopping; a shutdown that
could wait for a job holding a token before the function's later cancel is
rejected, and `with ThreadPoolExecutor(...)? as pool, stop.cancel_on_exit():`
cancels it on every exit. See
[cancel on every exit](cancellation.md#cancel-on-every-exit).

Workers must finish their communication before the scope can close. See
[futures](futures.md) for result ownership and
[parallel operations](parallel-map.md) for batched work.

## Submit from workers

A worker cannot hold the pool: it could wait for its own queue, its own
shutdown, or a future only it would run. `pool.submitter()` returns a
`Submitter` instead, a handle that can only submit without waiting. It can
enter workers, channel messages, and [`Shared[T]`](../01-builtins/shared.md).

| Operation | Result | Behavior |
| --- | --- | --- |
| `pool.submitter()` | `Submitter` | Another handle to the pool's queue |
| `submitter.share()` | `Submitter` | Another handle to the same queue |
| `submitter.submit_nowait(job)` | `Result[(), SubmitError[F]]` | Queue an owned zero-argument job, or return it |
| `submitter.submit_nowait(function, arguments...)` | Same, with a bound job type | Bind a statically named function and owned arguments |

```plenty
from std.atomic import Atomic, Ordering
from std.time import milliseconds, sleep

def visit(submitter: Submitter, visited: Shared[Atomic[u64]], depth: i64) -> ():
    if depth > 0:
        for _ in range(2):
            handle = submitter.share()
            counter = visited.share()
            child = def once [handle, counter, depth]() -> ():
                # depth > 0, so this cannot overflow.
                unchecked:
                    visit(handle, counter, depth - 1)
            match submitter.submit_nowait(child):
                case Ok(_):
                    pass
                case Err(SubmitError.Full(job) | SubmitError.Shutdown(job) | SubmitError.OutOfMemory(job) | SubmitError.CapacityOverflow(job)):
                    job()
    _ = visited.fetch_add(1, Ordering.Release)

def main() -> Result[(), AllocError | IoError | PoolError | SubmitError[()]]:
    visited = Shared[Atomic[u64]](Atomic(0))?
    with ThreadPoolExecutor(3, 2)? as pool:
        submitter = pool.submitter()
        submitter.submit_nowait(visit, submitter.share(), visited.share(), 4).drop_job()?
        # Recursive submission must finish before the pool shuts down.
        while visited.load(Ordering.Acquire) < 31:
            sleep(&milliseconds(1))
    print(visited.load(Ordering.Acquire))?
    Ok(())
```
```output
31
```

A `Submitter` job returns `()`. It has no future, so a result or error would
be dropped unseen; handle errors in the job, or send them on a channel. There
is no blocking `submit`: a worker waiting for space in its own pool's queue
could wait forever. A full queue returns `Full(job)` with the whole unstarted
job, which the caller can run inline, as above, retry later, or drop with
`drop_job()`. Waiting, results, cancellation, and shutdown stay with the
`ThreadPoolExecutor`; `pool.cancel_pending()` also cancels queued jobs that a
`Submitter` submitted, dropping their captures.

Shutdown closes the queue and drains the jobs it accepted. A `Submitter`
submission after that, including one from a job running during the drain,
returns `Shutdown(job)`. Recursive work must therefore reach quiescence before
the pool's owner leaves its block: wait until every job has reported, as the
example counts visited nodes, or the jobs submitted after the close are never
queued. The owner's drop shuts the pool down even while submitters live. A
`Submitter` keeps only the queue's memory, which the last handle frees; its
drop never waits.

Taking or sharing a handle updates a reference count and allocates nothing.
Each accepted job needs one fallible cell, as for `pool.submit_nowait`. A trap
in a submitted job stops the program, as in any pool job.

### Waits in submitting jobs

A job that can submit through a `Submitter` cannot wait, without a time limit,
for a job it queued: if every worker waits so, the jobs that would release them
stay queued behind them. The compiler applies a rule matched by message type,
as the shutdown rules are:

> A pool job that can submit through a `Submitter`, directly or through jobs it
> submits, cannot wait without a time limit for anything a job it can submit
> can reach. It cannot, with an untimed operation:
>
> - receive or select on a queue whose sender such a job can reach, or send on
>   a queue whose receiver such a job can reach;
> - wait on a `CancellationToken` when such a job can reach a token;
> - lock a `Mutex[T]` when such a job can reach a `Mutex[T]`.
>
> A timed form is allowed: one whose timeout or deadline always ends the wait.
> A token alone does not count. Locks have no timed form. A thread the job
> joins counts as part of the job: its waits are checked the same way.

The timed forms are `recv_timeout_ms`, `send_timeout_ms`,
`select_recv_timeout_ms`, `std.sync.select_recv_before`, and
`wait_timeout_ms` (see [timed selection](timed-selection.md)). The
[readiness](../../stdlib/ready.md) waits always take a deadline, so a job may
wait for a socket until a deadline or its token, as a crawler does.

A timed wait cannot hang the pool, but it has a cost: a cancellation or
message that a queued job would deliver arrives no sooner than the timeout,
since that job runs only once the wait ends. A loop that retries a timed wait
until a submitted job acts can still hang, as a loop polling with
`recv_nowait` or spinning on an atomic can; no static rule catches such a
livelock.

The rule does not cover socket or file I/O. A plain `TcpStream` read or write
waits without a limit, so when a queued job holds the peer, use the stream's
`read_before` or `write_before`, which end at a deadline or the token (see
[networking](../../stdlib/net.md)).

Everything is matched by type, as the shutdown rules match it. A job may
still send blocking on a queue whose receiver only another thread holds, as
the [tutorial lesson](../../../tutorial/11-concurrency/submit-from-workers.md)
does. The `_nowait` operations, `is_cancelled()`, atomics, and other I/O are
accepted. To share counters or flags with submitted jobs, use an
[atomic](../../stdlib/atomic.md) rather than a mutex.

A job waiting on something no submitted job can reach is outside the rule;
such protocols can still deadlock, as for any pool job. Code started on a
scoped thread, or `main`, is not a pool worker, so it may submit and then wait.

For the [shutdown rules](#shutdown-and-live-receivers), a job that can
submit through a `Submitter` can also hold every sender, and reach every mutex,
that a job it can submit can. A job that holds a `Submitter` but never sends
blocking still blocks a shutdown while a live receiver remains, when a job it
submits can send blocking on that receiver's queue.

The rule follows calls and closure calls; a closure made in a job runs where it
is called. It does not see submissions in a function compiled elsewhere, and it
matches queues by type, not identity.

## Shutdown and live receivers

Shutdown waits for every accepted job, and a job waiting for queue space is
released only by a receive or by the drop of the last receiver. The compiler
therefore applies the [task join rule](channels.md#joins-and-live-endpoints) to
pools:

> A pool cannot be shut down while a live binding of the function can hold
> `Receiver[T]`, once the function has submitted a job that can hold `Sender[T]`
> and [can wait](channels.md#which-workers-can-wait).

A job that [cannot wait](channels.md#which-workers-can-wait) for another
thread, such as one that sends only with `send_nowait`, therefore does not
block a shutdown.

A pool shuts down at `pool.shutdown()`, including through a
`&ThreadPoolExecutor`, at `drop(pool)`, when its binding is reassigned, and
wherever its owner leaves scope: the end of its `with` block or of the function,
`return`, `?`, `break`, and `continue`. A call that receives a pool, by value or
by reference, counts as a shutdown when a function it reaches, directly or
through further calls, calls `shutdown()` or owns a pool. Unknown calls are
conservative, with a bounded exception for a capture-free `Callable` whose
input and result types cannot expose pool authority or job hazards. For example,
calling an unrelated `Callable[[], ()]` no longer makes a submission-only helper
count as shutting down a pool, whether it takes `&ThreadPoolExecutor` or
`Submitter`. This does not resolve the callback's actual target. Plain data and
`Submitter` carry no shutdown authority; borrowed and aggregate types are
searched recursively. Pool, channel and mutex capabilities, destructor-bearing
classes, erased values and unclassified types remain conservative. Known closure
bodies are followed; unresolved closures, boxed/protocol calls and missing
function bodies remain unknown. Future-result wait checks are unchanged.

A rejected helper call reports whether shutdown was **discovered** (an explicit
shutdown or owned-pool cleanup) or **assumed** (an unresolved call). Notes identify
the enclosing function, available source positions and the call chain, including
missing bodies and erased callbacks. Concrete witnesses take precedence over
unknown calls. Chains show at most eight edges nearest the cause, with an omitted
edge count for longer paths. A passed pool binding is identified when available;
this is not proof that the shutdown and submitted job belong to that same pool.
The same explanations apply to helpers handed only job inputs or a `Submitter`.

Explicit shutdowns and owned-pool cleanup still count, even for an unrelated
local pool. These summaries do not match pool identity or prove call order or
general deadlock freedom. Locals are released last declared first, so a receiver
bound after the pool is dropped before the pool shuts down.

Jobs are not matched to pools and are never assumed to have finished. A job
submitted with `submit` or `submit_nowait` counts at every later shutdown in the
function, and at every shutdown in a loop that contains the submission. A
`submit(...)?` or `submit(...).drop_job()?` that fails has submitted nothing on
the path that leaves. A call that receives a pool, by value, by reference, or
borrowed from a field such as `&holder.pool`, counts as submitting, at the call,
every job that a function it reaches submits directly or through further calls,
to that pool or any other. A shutdown inside that call sees those jobs too.
A call handed no pool still counts as a shutdown at the call when a function
it reaches submits a job and can shut a pool down: the job may run in a pool
the callee makes and drops before returning. Only the jobs' senders and
mutexes of types that the call's arguments, or a closure's captures, can
reach count, since a job of the callee's own pool can hold a caller's
endpoint only through them. A helper that drains a channel it makes itself is
therefore accepted beside a caller's receiver of the same type.
`map`, `map_result`, and `reduce_tree` finish their jobs before returning and do
not count. What a job and a binding *can hold* is defined as for tasks.

The [recommended patterns](channels.md#recommended-patterns) for tasks apply.
When the function does not wait for the job's result inside the block, guard
the receiver as the last manager of the pool's `with`, so every exit drops it
before the pool shuts down:

```text
with ThreadPoolExecutor(2, 8)? as pool, receiver as inbox:
    pool.submit(produce, sender).drop_job()?
    ...
```

Receiver-last also leaves the guard alive during a helper call inside the
block. It cannot prevent a mid-call shutdown from waiting for a producer that
needs that receiver; draining later is too late. The
[helper-based scheduler lesson](../../../tutorial/11-concurrency/scheduler-loops.md)
shows an accepted submit/receive loop and cleanup on early exits. Other receiver
aliases must also be released, and workers must handle disconnection.

That guard is still alive at a `future.result()` inside the block, which
[its own check](futures.md#result-waits-and-live-receivers) rejects. To wait for
the result, end the receiver's guard first:

```text
with ThreadPoolExecutor(2, 8)? as pool:
    future = pool.submit(produce, sender).drop_job()?
    with receiver as inbox:
        ...
    result = future.result()
```

A channel created inside the pool's block, and a `drop(receiver)` on every path
to the shutdown, also satisfy the rule. The
[holders](channels.md#what-can-hold-an-endpoint) of a task join apply too: a
returned `Ok(receiver)`, an unfinished `pair = (receiver, future.result()?)`, or
a `match` case whose value is `receiver` holds it through the shutdown.
The diagnostic reports the shutdown or the pool's binding, each binding,
returned value, or unfinished expression that holds a matching receiver, where
the job is submitted, and the early exit that reaches the shutdown.

The rule has the task rule's
[false positives and gaps](channels.md#what-the-rule-does-not-decide). It also
rejects these safe programs:

- the job has finished, for example after `future.result()` returned;
- the job was submitted to a different pool than the one shutting down;
- a `submit` failure handled without `?` returned the job to the caller;
- a call handed no pool submits a job and then returns its pool, or shuts
  down only a pool it made before submitting.

It does not cover:

- a job submitted by a function compiled elsewhere or reached through a
  callable value;
- a job submitted by another function to a pool it returns;
- a pool passed to a call inside a class, tuple, or collection;
- a `map`, `map_result`, or `reduce_tree` callback that waits to send.

Blocking [future retrieval](futures.md#result-waits-and-live-receivers) has its own
check at `result()`, before any later receiver cleanup.

## Helper-based schedulers

Ordinary helpers may submit and receive while the owning pool stays alive.
The [shutdown contract](#shutdown-and-live-receivers) describes the bounded
capture-free `Callable` exception for borrowed-pool and `Submitter` helpers.

Capability-bearing or erased unknown calls, missing bodies and ownership of an
unrelated pool can still combine with transitive submissions to reject a safe
helper. This remaining limitation is not a requirement to inline all loops.

Read the diagnostic's discovered-versus-assumed shutdown provenance and
bounded call chain before restructuring. They explain one cause, not every
possible cause or proof of runtime execution.

Keep a narrow submission helper when this limitation requires separating setup
and bookkeeping from the pool-handed call. The
[scheduler lesson](../../../tutorial/11-concurrency/scheduler-loops.md#when-the-current-checker-rejects-a-larger-helper)
explains that fallback and preserving partial-preparation and typed-error
contracts. Cancellation does not interrupt ordinary blocking sends; use
[cooperative cleanup](cancellation.md) and keep endpoint/lock release before joins.
Capacity equal to worker count alone does not bound outstanding messages from
both queued and running jobs; see [submission and completion capacity](#submission-and-completion-capacity).

The aggregate-argument gap above is distinct from direct field borrowing:
`helper(&holder.pool, ...)` hands the call a pool, while passing only
`helper(&holder, ...)` can miss recording submissions through its pool field.
A callback whose shutdown effect remains unknown, such as an erased or
capability-bearing callback, can nevertheless trigger rejection in the latter
helper through the callee-owned-pool check. Neither wrapping a pool nor observing
that one wrapper example rejects establishes safety or complete coverage.

## Shutdown and held locks

A job waiting for a mutex this function holds would keep the shutdown waiting
too. Submission therefore also records the guarded types of the `Mutex[T]`
values a job can reach, including through `Shared` handles:

> A pool cannot be shut down while the function holds a lock on a `Mutex[T]`,
> once it has submitted a job that can reach a `Mutex[T]` of the same `T`.

Shutdown points, job counting, and the false positives are those of the
receiver rule; mutexes are matched by type, as for
[joins under a lock](mutex.md#waits-the-compiler-rejects). End the lock's
`with` block before the pool shuts down.

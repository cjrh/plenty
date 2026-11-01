# Blocking work for actors

Status: **Proposed; not implemented.** This is a design for the blocking-work
part of B85, not a new scheduling or thread API available today. Priorities and
remaining scope live in the [backlog](../backlog.md). The companion pages cover
[ownership and failure](actor-blocking-work/ownership.md) and
[runtime integration and qualification](actor-blocking-work/runtime.md).

## Purpose and existing limits

An actor HTTP handler should be able to call an existing blocking HTTP client,
database driver or file operation without stopping unrelated connections.
The callback submits work, returns to the stage, and later receives a completion
event. Waiting occupies a worker thread, not the stage thread.

Today [actor callbacks](../design/stdlib/actor.md#actors-and-stages) run to
completion on one thread. Ordinary thread creation inside a callback traps;
scoped threads also join before scope exit. Actor nodes cannot retain ordinary
thread, channel, executor or shared-state handles. Wrapping `spawn` or a future
in a Plenty class cannot bridge these constraints safely. The existing
[actor DNS lookup](../design/stdlib/actor-net.md#outbound-connections-and-lookup)
already demonstrates helper-thread completion independent of an actor heap,
but is a specialized operation, not a general offload API.

Prefer [actor networking](../design/stdlib/actor-net.md) and
[actor TLS](../design/stdlib/actor-tls.md) for protocols that already have
nonblocking state machines. Offload is a compatibility path for blocking APIs;
it is not a promise that more threads improve throughput.

## Recommended API direction

Use an explicitly created, bounded OS-thread pool, owned outside the stage and
attached before the stage runs. No implicit global pool or thread per request.
The first version accepts named, capture-free worker functions and owned
`Bytes` request/result payloads. A typed error remains available; arbitrary
closures, borrowed arguments and generic object-graph transfer are excluded.

The following is a contract sketch, not executable Plenty or final generic
syntax. Suggested module: `std.actor.blocking`.

```text
Limits(max_workers: i64, max_jobs: i64, max_input_bytes: i64,
       max_output_bytes: i64, max_worker_bytes: i64)
limits() -> Limits
open_pool(limits: &Limits) -> Result[Pool, BlockingError]
Pool.attach(stage) -> Result[PoolKey, BlockingError]

worker(input: Bytes, deadline: &Deadline, cancel: &CancellationToken)
    -> Result[Bytes, E]

Context.submit_blocking(pool: PoolKey, worker, input: Bytes,
                        deadline: &Deadline)
    -> Result[Job[E], SubmitError[Bytes]]
Job[E].take_now(context) -> Result[Option[Result[Bytes, E]], BlockingError]
Job[E].abandon(context) -> Result[(), BlockingError]
```

`Pool` is not actor state. `PoolKey` is a checked non-owning capability scoped
to its attachment; `Job[E]` is a narrowly certified opaque actor handle. Neither
grants a worker access to `Context` or the stage. An attach operation must not
make `Pool` destruction race a live attachment. Final names and receiver syntax
must be checked against [stdlib conventions](../design/stdlib/conventions.md)
and the compiler's generic callback representation during implementation.

`submit_blocking` never waits for a free worker or queue slot. It checks the
pool/stage identity, input size and deadline, prepares an
independent input snapshot, and atomically admits a job. Function eligibility is
a compile-time check, not a run-time escape hatch. Admission errors return
the original input in `SubmitError[Bytes]`, with the reason (`Capacity`,
`Closed`, `InvalidPool`, `TimedOut`, `LimitExceeded` or allocation failure).
`BlockingError` embeds `Allocation(AllocError)` rather than an allocation union.
Worker errors `E` retain their type and cause; they are not flattened into text.

An accepted job reserves one of `max_jobs` records until its worker resources
and result are reclaimed, including after its caller abandons it. The bound
covers queued, running and completed-but-unclaimed jobs, not just the waiting
queue. Invalid sizes and unsupported configurations fail explicitly. Defaults
must be documented and measured before adoption; the API does not derive a
worker count from incoming traffic.

## HTTP handler experience

Use the [event HTTP interface](../design/stdlib/actor-http-streaming.md):

1. On request `End`, build a bounded `Bytes` description of the upstream request,
   submit a named worker and retain its `Job[E]` and application correlation ID.
2. Return without joining, polling in a loop or waiting for the result.
3. On `Event.Actor(Mail.Blocking(job_id))` (proposed), call `take_now` for the
   matching job, then build the response from its result.
4. On disconnect, request timeout or `Stopping`, abandon outstanding jobs.
   Apply the application's existing HTTP error policy for upstream failure or
   capacity exhaustion; do not automatically retry a side-effecting operation.

The worker constructs its own client/configuration, decodes input, invokes the
existing blocking fetch with the supplied deadline and cancellation token, and
returns bounded bytes or a transferable typed error. Do not claim every existing
client error is transferable: eligibility must be checked, with an explicit
owned error adapter where needed. The streaming interface means asynchronous
response timing; the initial offload API itself returns one bounded result,
not an unbounded response stream.

A database wrapper follows the same pattern but must satisfy the driver's
thread-affinity and effect contracts. Persistent database sessions and connection
pools need a separately designed worker-owned resource facility; an arbitrary
connection cannot simply move out of an actor into successive pool threads.

## Decisions and alternatives

A bounded pool amortizes thread creation and makes saturation visible. One OS
thread per request allows incoming traffic to exhaust threads and stacks.
Blocking `join` inside a callback defeats the purpose. Moving the blocking call
to another actor on the same stage also stalls that stage. Supporting several
stage threads would not make a blocking call cooperative and is independent.

Use byte payloads initially to avoid promising safe deep transfer of arbitrary
actor-owned values. A named function avoids storing actor captures in a job.
The compiler can reuse existing worker-effect checks, but must add an explicit
actor-to-worker boundary rather than exempting ordinary `spawn` from its guard.
Full typed requests, streaming results and retained worker resources require
separate contracts. This proposal does not introduce async/await.

# Blocking-work runtime and qualification

This records implementation guidance for the proposed
[actor blocking-work API](../actor-blocking-work.md), not a second work queue.
B85 in the [backlog](../../backlog.md) owns scheduling and outstanding scope.

## Runtime division

Implement policy, limits and the HTTP adapter examples in Plenty where practical.
A small native boundary is necessary for thread creation, independently owned
job storage, synchronization, allocator budgets, compiler-certified transfer and
selector wake-ups. Ordinary Plenty code cannot safely construct these lifetimes
from actor heaps and lexical thread handles. Reuse existing runtime thread and
selector primitives where their contracts fit; do not add public raw std hooks.

Before coding, map the specialized DNS helper, actor retirement, selector handle
tracking, native worker checks, callable lowering and allocator budget paths.
The generic bridge is new work, not a wrapper around DNS's private payloads.
An existing executor cannot be assumed safe to retain or destroy in an actor.
Sharing executor internals requires the same ownership and shutdown guarantees.

## Bounded registry and wake protocol

Allocate `max_jobs` registry entries and completion capacity when opening the
pool; validate overflow and allocation failures. Each entry has an identity,
state, owner generation, deadline, cancellation flag, transfer descriptor and
bounded payload storage. Reserve a small allocation-free infrastructure-error
outcome per entry, so failure to allocate an ordinary output can still notify
the actor. Opening a pool must roll back already-started workers on partial
startup failure. Native synchronization must have short critical sections;
never execute work, user cleanup, blocking I/O or wait for capacity under a
stage-visible lock. No stage operation waits on worker completion.

Use the following lifecycle, with abandonment tracked independently once running:

```text
Free -> Queued -> Running -> Completed -> Claiming -> Reclaiming -> Free
Queued -- cancelled before start --> Reclaiming
Running -- abandoned --> Running (no waiter) -- finished --> Reclaiming
Completed -- abandoned --> Reclaiming
```

A worker publishes its result before signalling the selector wake handle.
The signal is a hint to drain the registry, not the sole record of completion.
Coalesced or saturated wakes must not lose results. The stage drains a bounded
number of completions per turn and schedules another internal turn if records
remain; otherwise a completion flood can starve HTTP sockets and timers.

Deliver `Mail.Blocking(job_id)` through reserved control state, independent of
ordinary mailbox capacity and application timers. One reserved notification per
outstanding job is sufficient. Retry notification scheduling while the job is
live if necessary; do not silently discard it on `Full`. `take_now` may see a
pending job and return `Ok(Nothing)`; repeated reads after consumption or explicit
abandonment return a named error. Stale already-queued notices are harmless and
never read reclaimed storage. Actor retirement cancels all its registry waiters.

Budget planning must count registry metadata, independent input/output copies,
worker scratch allocations, actor-side result copies, thread stacks and native
library allocations separately. `max_jobs` bounds retained input/output records;
`max_workers` bounds concurrent scratch budgets and stacks. Queued input must
not also acquire a full worker budget before execution. Measure transient
transfer peaks and cleanup backlog, not just steady-state body sizes.

Ready results consume admission capacity until claimed or reclaimed, bounding
slow consumers. Scope attachment to one stage initially; one pool shared by many
stages needs additional fairness and wake-routing contracts. Permit separate
pools for different services so a stuck database need not exhaust HTTP-client
workers, with process-wide resource budgets remaining the application's concern.

## Lifetime and shutdown

The outer driver owns the pool and a stage attachment. An actor cannot close or
join the pool. HTTP serving needs an explicit optional pool attachment accepted
by event-server entry points or a server setup object; do not hide a pool in a
capture that violates the current factory/node rules. Preserve existing server
entry points and defaults. This integration/API shape is an implementation design
gate, not an assumed existing hook.

Expose nonblocking admission closure and explicit drain-versus-cancel modes.
During HTTP draining, application completion events continue alongside socket
progress until the grace deadline. After that, retiring connection actors
abandons their jobs and detaches their response destinations. The pool must
outlive running jobs; stage storage and wake handles cannot be freed while a
worker may still access them. Use independently retained completion state and
an explicit detach handshake, not borrowed stage pointers.

Final pool shutdown/join runs outside callbacks, with `deadline: &Deadline` and
`cancel: &CancellationToken` on a waiting API. If that wait times out, return an
error while retaining the owner for retry; it does not authorize freeing running
job storage. No implicit detached workers. Final owner destruction must join
remaining workers and can therefore wait indefinitely for uncooperative calls;
document this even when an explicit earlier wait was bounded. Do not promise a
bounded whole-process shutdown for arbitrary blocking libraries. Releasing a
stage attachment must only sever routing and signal cancellation, without joining.

## Evidence required for adoption

Use a controlled local upstream and barriers/latches rather than relying on
scheduler luck or an Internet endpoint. Tests should establish:

- An HTTP request waits on a blocked worker while unrelated connections complete,
  timers fire and cancellation is observed by the stage.
- `max_workers` bounds concurrent execution; `max_jobs` includes completed and
  abandoned-running work. Saturation returns original input without running it.
- Actor traps, ordinary stop, disconnect, slot reuse and stage destruction before
  completion leave no dangling reads, misdelivery, lost cleanup or leaked heaps.
- Full mailboxes, exhausted application timers, coalesced wakes and completion
  bursts preserve notification and fair progress without busy polling.
- Deadline expiry before start, completion/abandon races and shutdown while queued,
  running or ready produce one outcome and one cleanup. A call ignoring its token
  holds its slot until the controlled upstream releases it.
- Allocation failure at admission, worker execution, publication and actor result
  transfer is recoverable where specified and never publishes a partial job.
- Compile-fail tests reject actor captures, borrowed payloads, forbidden effects,
  unsafe error types and ordinary thread spawning in actor callbacks. Valid
  fetch adapters pass the normal worker checks without an exemption.
- Returned typed upstream errors survive transfer. Fatal worker behavior is
  qualified in a subprocess and documented independently of actor containment.
- Repeated create/run/drain/drop cycles return threads, descriptors and tracked
  allocations to baseline; race/sanitizer checks cover abandonment and wake teardown.

An executable tutorial should implement an actor HTTP endpoint backed by a local
blocking fetch, including capacity exhaustion, upstream failure, timeout and
shutdown. Measure latency of unaffected requests, queue delay, throughput, CPU,
thread count and peak memory against nonblocking actor I/O and the worker HTTP
server. Report actual platform coverage; support must follow native thread and
selector qualification rather than claiming portability from a Linux result.

## Questions to settle during implementation

The safety contracts above are requirements; representation details still need
prototypes: concrete generic error-transfer lowering, the bounded worker allocator,
reserved completion-mail scheduling, the stage attachment lifetime, HTTP server
pool injection, and errors/results that require thread-affine destruction.
The initial version may narrow eligible errors further if safe bounded transfer
cannot be established. Do not widen accepted types or drop allocation guarantees
merely to make the first fetch example compile.

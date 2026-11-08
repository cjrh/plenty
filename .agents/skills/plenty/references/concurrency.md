# Pool schedulers and shutdown

Start from the runnable [helper-based scheduler](../../../../book/src/tutorial/11-concurrency/scheduler-loops.md),
then check the authoritative [executor rules](../../../../book/src/design/facilities/03-concurrency/executors.md#shutdown-and-live-receivers).
Ordinary submit/receive helpers are supported; do not inline every scheduler by default.

## Lifetime and communication

- An owning pool drains accepted jobs and joins on drop and every scope exit,
  including `?`, `return`, `break`, and `continue`. Dropping a `Future` neither
  joins nor cancels its job. Explicit shutdown waits too.
- Guard the receiver **after** the pool, so cleanup releases it before joining.
  With a stop token, use `with ThreadPoolExecutor(...)? as pool,
  stop.cancel_on_exit(), receiver as inbox:`. Managers unwind in reverse order.
  Account for all receiver aliases and worker handling of disconnection.
  Release held mutex guards before joining.
- Receiver-last protects **scope exit**, not a shutdown or `future.result()`
  inside a helper while the receiver is still needed. A later drain cannot
  repair a shutdown already waiting for a blocked send.
- Cancellation is cooperative: ordinary `Sender.send`, child waits and foreign
  calls do not become interruptible merely because a token is cancelled.
  Use a suitable [cancellable operation](../../../../book/src/design/stdlib/sync.md#typed-deadline-waits)
  where needed; settle unsent messages/errors. Arrange cancellation, endpoint
  release and lock release before joining on every path.
- Count outstanding messages, including queued **and** running jobs and all
  producer aliases. Channel capacity equal to worker count alone is no proof.
  The checker does not prove such counts or recognize a drained set as finished
  jobs; it conservatively matches types, not queue/pool identities.

## Helper rejection and a narrow fallback

Capture-free `Callable` calls with recognized data-only input and result types
do not imply shutdown, for either borrowed-pool or `Submitter` helpers.
Recognized signatures may also contain `Submitter`; borrowed and aggregate
contents are checked recursively. Capability-bearing or erased unknown calls,
missing bodies and ownership of an unrelated pool can still combine with
transitive submissions to reject a safe helper. The bounded exception does not
resolve callback targets or relax future-result wait checks.

Read the shutdown provenance: **discovered** identifies explicit shutdown or
owned-pool cleanup; **assumed** identifies an unresolved call, not proof that it
shuts down. Follow the bounded call-chain notes and available source positions,
inspect reachable calls and reduce the reproduction before restructuring.
Concrete witnesses take precedence over unknown ones, but even a discovered
shutdown need not belong to the submitted job's pool or run after submission.
See [current helper limitations](../../../../book/src/design/facilities/03-concurrency/executors.md#helper-based-schedulers).

If that limitation blocks a helper, keep a small pool-borrowing `submit_run`
that only submits. Separate `prepare` and `hand_off` bookkeeping from that
call; place the scheduling loop in the pool-owning function only where needed.
The Vampire case study used this split, plus `plan`/`await_one` and caller-owned
partial preparation output. Its successful workaround does not establish that
all submit/receive loops require inlining.

Preserve [partial-preparation obligations](../../../../book/src/tutorial/11-concurrency/scheduler-preparation.md):
when later setup fails, earlier prepared resources must remain owned and get
handed off or cleaned up. An error while appending the current run needs cleanup
too. Preserve typed setup/allocation errors; a failed copy is not automatically
a generic failed-run outcome. Successful submission and failed bookkeeping
also need an explicit accepted-job cleanup path.

Do not hide a pool in an aggregate to evade diagnostics. Passing
`helper(&holder.pool, ...)` records the direct pool argument; passing only
`helper(&holder, ...)` has a known submission-tracking gap. Another analysis
path may still reject the latter for a callback whose shutdown effect remains
unknown, such as an erased or capability-bearing callback. Acceptance is not
proof of safety, and neither behavior is an escape hatch.

## Submission authority and errors

[`Submitter`](../../../../book/src/design/facilities/03-concurrency/executors.md#submit-from-workers)
is a restricted, shareable submission capability, not an owner. It cannot shut
down, wait for queue space or produce futures; its jobs return `()`, and
`submit_nowait` returns `Result[(), SubmitError[F]]`. Owner shutdown closes the
queue even while handles remain, so recursive work must reach quiescence first.
The data-only `Callable` case is accepted for Submitter-only helpers too.
Other unknown effects and unrelated owned-pool cleanup remain conservative,
so changing the parameter alone is not a universal workaround.

On rejected submission, the error owns the unstarted job. Retain it for
recovery, or use `.drop_job()` deliberately to release captures while keeping
the `SubmitError[()]` variant (`Full`, `Shutdown`, `OutOfMemory`, or
`CapacityOverflow`). On success the pool owns the job; do not await a completion
for a rejected one. Preserve worker errors in a future or completion message.

Do not ignore `Full` or busy-retry `send_nowait`/`submit_nowait` to satisfy a
check. They change delivery/backpressure semantics; silent loss and infinite
retries are not shutdown solutions. Blocking pool submission never returns
`Full`, but can itself deadlock if the scheduler submits indefinitely instead
of receiving completions. Design the protocol, then check it.

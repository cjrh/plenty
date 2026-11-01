# Blocking-work ownership and failure

This accompanies the proposed [actor blocking-work API](../actor-blocking-work.md).
None of these contracts are implemented yet.

## Three allocation domains

An actor's heap can disappear after any callback traps. Moving a `Bytes` header
into a native job does not make its backing storage independent. Submission
must copy the payload into a separately owned job allocation before publishing
it to workers. The copy is explicit in the contract and bounded by
`max_input_bytes`; failure leaves the original input recoverable. Metadata,
cancellation state, wake handles and error storage must likewise contain no
pointers into an actor heap.

Workers use ordinary thread-owned allocations under a proposed per-job
allocation budget, never the actor heap table. The worker creates its own client,
TLS configuration, request strings and any foreign handles. Native code must
not temporarily switch a background thread into an actor's heap. The existing
single-owner actor heap rule remains intact.

Completion retains an independent result until the stage claims it. Only the
stage thread copies the result into the receiving actor's heap. Output bytes
and typed errors together must fit `max_output_bytes`; transfer can also fail
if that actor has insufficient space. Report a distinguishable transfer error
when recoverable; an actor allocation trap must still retire the actor and
reclaim the independent completion. No worker frees or reads retired actor
storage, and no actor owns a pointer into a worker's temporary stack.

`max_worker_bytes` requires real enforcement in the Plenty allocator path,
including worker-created client state and reallocation. Merely limiting the
returned body does not bound allocations made while fetching it. Reuse or extend
allocation-failure propagation so budget exhaustion is a recoverable job failure;
existing actor trap/OOM machinery is not a worker implementation. Audit the
client's header/body/decompression limits as well. Foreign allocations and OS
thread stacks are outside this allocator budget and must be documented, bounded
where the wrapped API permits, and included in qualification measurements.

## Eligibility and API capability

The first version uses capture-free named functions. Apply the existing
[worker eligibility checks](../../design/facilities/03-concurrency/index.md)
to the function, transitive calls, errors, results and destructors. Owned bytes
do not make an otherwise forbidden foreign effect safe. Reject borrowed state,
uncertified indirect effects, actor addresses used as cross-thread senders,
actor heap operations, executor submission and worker-side future waits.

`E` must additionally be independently owned, bounded and transferable into an
actor node under the actor type rules. Derive/check the transfer operations from
concrete types; do not introduce an unchecked public marker or raw-pointer API.
Restrict unsupported error types with a useful diagnostic. Account for the size
of any owned error payload, and retain the cause through an explicit adapter.
A byte-only input/result API does not silently authorize shared strings, locks,
channels or arbitrary handles in actor nodes.

The only new actor-storable types are the certified job handle and pool key.
A handle identifies a registry entry; it does not own a native thread or a
shared allocation directly. Dropping it marks the waiter abandoned without
joining. Trap retirement must do the same through stage bookkeeping, because
user destructors do not run on contained traps. Pool shutdown must remain safe
even if no user-level job handle destructor ran.

## Completion identity and cleanup

Use the stage attachment identity, actor slot generation and job generation
when routing a completion. A recycled actor slot, new stage instance or new
HTTP request must never receive an old result. Validate the result's concrete
error/transfer type against the job record before decoding it.

Exactly one owner wins a claim-versus-abandon race. Claimed results are consumed
once; failed transfer destroys the source once. Abandoned queued jobs are removed
before execution when possible. Abandoned running jobs retain their slot and
cancellation state until they finish, then destroy output and release resources
without posting to the retired actor. Cancellation cannot promise that an
accepted operation has had no external effects.

Cleanup of worker-owned values, including abandoned results and errors, runs on
a worker/reaper path whose lifetimes and capacity are bounded by the pool.
Unbounded user cleanup must not run on the stage thread. The first version's
payload/error restrictions must permit bounded transfer and cleanup on the actor
side. Reclamation cannot require a new queue allocation or mailbox slot.

## Deadlines, cancellation and reliability

Submission takes an absolute `Deadline` and copies its monotonic value. Queue
waiting counts against it. Expired queued work must not start. The supervisor
signals a worker-owned cancellation token when the job is abandoned, its deadline
passes or the pool starts cancelling. A worker may inspect that token or pass it
to a cooperating library. The actor does not retain a shared token in its node.

Stopping the wait is prompt; stopping a blocking operation is cooperative.
A foreign call that ignores cancellation can continue after the HTTP request,
actor and server have ended. Its slot remains occupied, so repeated timed-out
requests cannot create unlimited replacement threads. The registry separates the actor's terminal wait outcome from physical job
completion: deadline expiry can notify the actor with `BlockingError.TimedOut`
even while an uncooperative worker retains the job slot. No new per-expiry job
record is allocated. Late worker output is discarded after that outcome wins.

Serialize outcome publication, deadline expiry and abandonment under registry
synchronization. At publication, compare the monotonic clock with the copied
deadline: a result published at or after expiry loses to timeout, even if the
stage has not processed its timer yet. A result published before expiry remains
a result if the actor observes it later. Explicit abandonment suppresses further
user notification and makes an already-queued notice stale. Tests must cover
these orderings with controlled time and barriers rather than scheduler luck.

An ordinary returned `Err(E)` is a job outcome. A late or abandoned result is
reclaimed. Neither actor supervision nor thread isolation contains arbitrary
worker traps, stack overflow, native crashes or process-wide lock corruption.
The first version retains ordinary native worker fatal-trap semantics and must
say so prominently: code that would be contained inside an actor may instead
terminate the process when offloaded. Do not catch a runtime panic and report a
successful containment guarantee. Use a separate process when that isolation is
required. External database writes and HTTP side effects are never rolled back.

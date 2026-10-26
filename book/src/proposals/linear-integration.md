# Linearity, typestate, and execution responsibility

Status: **Not implemented; integration assessment for issue #149.**
Planning remains in [B72](../backlog.md). This page connects
[linear obligations](linear-obligations.md),
[resolution scopes](linear-scope-resolution.md), and the existing
[typestate exploration](typestate-and-owned-transitions.md); it does not approve
an effects or concurrency redesign. Examples below are speculative pseudocode.

## Recommendation and boundaries

Use small proposals with explicit interfaces: a linear ownership core (including
its inseparable aggregate/error rules), a separate synchronous resolution-scope
extension, and this integration contract. Keep the existing typestate exploration
independent. No new effect syntax is required for concrete synchronous consumers.
Propose effect-polymorphic resolution or suspension only when a concrete use
cannot fit current certification or execution ownership.

| Organization considered | Assessment |
| --- | --- |
| One linear-types proposal, existing typestate exploration, narrow effects section | Viable for manual synchronous resolution. Automatic scope policy and future execution ownership would make the document carry separable decisions. |
| Separate linearity/typestate/effect extensions with a small common contract | Preferred where there is an actual extension. Keep typestate's current assessment; split scope policy now. Do not manufacture an effects extension when reachable-call checking suffices. |
| Combined linearity, typestate, effects, and concurrency proposal | Shared examples help, but there is no demonstrated semantic need for one adoption unit. It would force syntax/runtime decisions that synchronous linearity does not depend on. |

The pages form independent review units where possible, not claims of complete
implementation independence. Aggregate cleanup and failure ownership cannot be
deferred from the linear core: a single discarded wrapper defeats the guarantee.
Scope resolution depends on core preservation and terminal authority. Worker
support depends on auditing runtime discard paths. Async completion depends on
a responsible execution owner and suspension rules; it is not a core prerequisite.

## Responsibility and dependency matrix

| Responsibility | Implemented baseline | Proposed owner / dependency | Deferred |
| --- | --- | --- | --- |
| Ownership and permission to discard | Affine moves, borrows, destruction | Linear core adds non-discardability, seal transfer, and checks at every cleanup edge | General user-defined trusted terminal declarations |
| Legal state transitions | Nominal classes, consuming functions/methods, runtime enums | Existing typestate exploration; linear states additionally preserve seals | State-specialized generics |
| Permitted effects / worker eligibility | Reachable native-effect walk, concrete callable/destructor checks | Reuse it for terminal calls and inserted resolvers; no stdlib exemption | General indirect-effect certification / effect polymorphism |
| Cleanup policy | Deterministic fallback drop, unit borrowed `__exit__` | Separate consuming scope resolver; policy explicitly named | Fallible exit syntax and async destructors |
| Execution responsibility / suspension | Native worker retention, joins, cooperative cancellation | Initially restrict discard-capable APIs; new completion contract needs runtime ownership audit | Suspended computation ownership and driving async cleanup |
| Failure ownership | Result payload transfer, lossy `Failure` erasure, owned submission errors | Linear core preserves live owners in errors and allocation rollback; each API defines terminal versus retry outcomes | Automatic recovery policy and remote-state certainty |

## Counterexamples that test the split

A synchronous transaction with `commit(tx)` and `rollback(tx)` can carry a
linear obligation using one nominal type and consuming free functions. It needs
neither state-specialized methods nor effect annotations. Conversely,
`prepare(Draft) -> Ready` is useful affine typestate even if discarding a draft
or prepared value is harmless. Linearizing every state would add obligations
without a corresponding product requirement.

Effects determine where an operation may execute, not whether it eventually
executes. A worker-certified `rollback` may block forever; a locally permitted
main-thread-only resolver can be meaningful even though worker certification
rejects it. A function marked consuming is still not a terminal operation unless
its ownership contract accounts for the seal and all failure paths.

## Linear typestate with explicit errors

Consider a protocol whose contract requires a response or completed local
cancellation after acceptance:

```text
send(Request) -> Result[InFlight, SendError]
SendError = NotAccepted(Request, Cause) | Uncertain(InFlight, Cause)
receive(InFlight) -> Result[Response, ReceiveError]
ReceiveError = Retry(InFlight, Cause) | Finished(TerminalFailure)
request_cancel(InFlight) -> Result[Cancelling, CancelStartError]
CancelStartError = Retry(InFlight, Cause) | Finished(TerminalFailure)
finish_cancel(Cancelling) -> Result[Cancelled, CancelFinishError]
CancelFinishError = Retry(Cancelling, Cause) | Finished(TerminalFailure)
```

Here `Request` is affine before acceptance; `InFlight` and `Cancelling` are
linear and carry the same obligation through transitions. `Response`,
`Cancelled`, and `TerminalFailure` are ordinary values because this *example's*
contract considers their outcomes terminal. Another protocol may require a
linear response body or recovery token instead. Uncertain acceptance cannot be
reported as definitely unstarted when work may already exist remotely.

Typestate rejects calling `finish_cancel` on `InFlight` or receiving after the
handle becomes `Cancelling`. Linearity rejects simply returning after a cancel
request or discarding `Retry(Cancelling, cause)`. Either mechanism alone misses
the other's error: an affine state may be abandoned; one linear runtime-state
handle may still reject operation order only at runtime. Together they prevent
abandonment and statically restrict these transitions, without proving that
`finish_cancel` ever returns or that a remote peer received the cancellation.

Constructors must not let clients forge accepted or cancelled states. Moves
prevent reuse of old handles, and terminal authority defines allowed outcomes.
Use consuming free functions in experiments; current ordinary methods borrow
receivers. For queues and runtime routing, an enum owning either linear state
may be simpler than carrying state parameters through every helper. Input-driven
parsers generally still suit runtime enums. See the existing typestate page for
HTTP response lifecycles, field-extraction friction, and SQLite retry behavior.

## Worker effects and hidden cleanup

Suppose an explicitly chosen resolver restores a process working directory:

```text
restore(scope: DirectoryObligation) -> ():
    while True:
        match std.env.set_current_dir(scope.saved_path):
            Ok(()):
                authorized_restored(scope)
                return
            Err(cause):
                record_error(cause)           # retain scope and retry

worker(owner: DirectoryObligation):
    with resolving(owner, restore):
        compute()
```

This worker must be rejected: its required exit reaches a process-global effect.
The example deliberately retries restoration failures and may never return;
it does not silently treat failed restoration as success.
The current [effect checker](../design/facilities/03-concurrency/effects.md)
already follows ordinary calls and destructors; generated consuming resolver
calls must be visible in that same graph. A diagnostic should identify the
submission/spawn, captured obligation, resolution scope, and call chain to
`set_current_dir`, with the existing reason that it affects every thread.
Report an ownership error as well if a proposed alternative merely drops the
owner to avoid the forbidden call.

Do not classify all linear owners as main-thread-only. In principle a worker
can compute while borrowing the data and leave resolution with a parent, or
return the obligation through an ownership-preserving join protocol. Reject at
worker certification when the reachable body/required cleanup needs the effect,
and at transfer when the chosen worker API cannot preserve ownership on all
paths. For the initial core, existing APIs that can discard such owners are
ineligible regardless of whether the particular body appears safe.

Concrete resolvers with known bodies need no new effect annotations. A generic
resolver specialized to a concrete callable can use existing certification only
where the resulting calls are inspectable. Current indirect and boxed callable
restrictions still apply; an erased resolver or unresolved higher-order call
cannot claim worker safety just because its type says it consumes an owner.
Such use needs either a statically inspectable adapter or a separately justified
effect-contract extension. The limitation must appear at the requiring worker
use and the hidden resolver call, not as an exemption for library cleanup.

## Native workers and future completion

Current [Future](../design/facilities/03-concurrency/futures.md) is affine.
Dropping it neither cancels the job nor waits: accepted running work retains a
worker owner and proceeds. Executor shutdown joins running workers, and ordinary
shutdown drains accepted queued jobs. Cancellation of running work is
[cooperative](../design/facilities/03-concurrency/cancellation.md); setting a
token does not force termination or change a future's result by itself.

These facts are useful execution machinery but do not make every stored value
linear-safe. Preserve today's APIs for discardable data. The first linear core
should reject linear captures/results in existing thread, executor, channel,
and lazy-frame paths that can destroy them, until a path-complete contract is
implemented. This is a proposed restriction for new types, not a claim that
today's compiler knows `Discardable`.

| Existing path | Obligation-preserving extension needed before admitting linear values |
| --- | --- |
| Thread-start failure destroys moved inputs | Return complete inputs or invoke an authorized failure resolver. |
| Executor submission fails | `SubmitError[F]` already returns the complete job; preserve its captures, reject lossy erasure/drop, and audit allocation before transfer. |
| Queued job cancelled / `shutdown(True)` | Current cancellation skips execution and later destroys captures. Return them to a responsible owner or run a certified resolver; marking the future done is not resolution. |
| Job runs normally | Its body and reusable-environment cleanup must resolve/transfer every capture, on success and failure. |
| Public future dropped / result unclaimed | Current runtime may destroy the result. Require a new non-discardable result handle or an explicit runtime-owned result resolver. |
| Scope exits / executor shuts down | Joining proves workers returned, not that a returned obligation was handled. Retained result storage needs a continuing owner. |
| Channel disconnect, queued-message cleanup | Return or resolve linear messages; destroying a queue is not enough. |

A possible `Completion[T]` would specify those responsibilities explicitly;
it must not silently redefine `Future[T]`. A joinable synchronous implementation
could precede async/await. Submission failures preserve the job; cancellation
must return it or complete its resolution; terminal results transfer to the
consumer or an authorized resolver. All hidden resolvers require effect checks
on the thread where they actually run, and must run without scheduler locks.

In future async work, moving a completion into `.await` merely transfers the
obligation to the suspended frame. Dropping that frame cannot abandon it. A
responsible scope/runtime must continue driving the computation or its
cancellation to an allowed terminal outcome. `async with` or an async destructor
provides a place to write suspending cleanup; neither ensures someone keeps
executing it. The separate [async/await exploration](async-await.md) now
examines suspension and execution ownership, but its affine spike does not
support linear values or establish non-abandonment. Such syntax still needs a
complete obligation-preserving runtime contract before claiming that guarantee.

A timeout can return immediately only by retaining responsibility elsewhere or
returning the live owner. The responsible scope must retain all required owned
resources and outlive any retained loans; a background cleanup cannot use a
borrow into a caller that already returned. Completion can still wait forever,
deadlock, or fail remotely. No fairness, termination, crash recovery, or bounded
cancellation guarantee follows from linear ownership.

## Integration checks before adoption

Demonstrate a synchronous linear transaction without new typestate/effect
syntax, and an independently useful affine typestate API. Test the protocol
above for illegal order, dropped retry states, and legal ownership-preserving
retries. Worker compile-fail cases must include direct consumers, helper chains,
inserted exit calls, and uncertified indirect resolvers. Positive worker tests
must use permitted effects and an audited transfer protocol.

For every new worker protocol, inject start/submission failure, cancel queued
work, drop the result consumer, exercise shutdown with pending/running work,
and verify captures/results are resolved or returned exactly once. Include
timeouts that return a live owner and reject a scope escaping required loans.
Do not use a successful join or a cancellation flag as evidence of protocol
completion. These checks bound any later implementation; this proposal adds
no runtime or language behavior.

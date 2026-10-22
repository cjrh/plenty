# Exploring Go-style tasks with Plenty's ownership rules

Status: **Not implemented; exploratory proposal with a standalone native
spike.** Lightweight tasks are neither adopted nor scheduled for Plenty.
The [backlog](../backlog.md) records the investigation; the
[concurrency reference](../design/facilities/03-concurrency/index.md) describes
the existing native workers. The production compiler, runtime and tutorial are
unchanged. Keeping the current concurrency model is a valid outcome.

## The question and provisional answer

Can ordinary functions suspend on communication, retain their local borrows, and
clean up synchronously, without an exposed pinning API or async destructors?
**Scoped tasks on immovable, fixed-size stacks look like a plausible smaller
model.** The [spike](stackful-tasks/evidence.md) exercises actual stack switching,
nested calls with stack-local references, bounded communication, cancellation and
fallible admission. It does not establish a production implementation or the
scalability normally associated with Go.

This is simpler at the function boundary than movable async frames: the call
stack retains ordinary locals and nested calls; a callee can keep borrowing its
caller's local across a suspension. The simplification costs a stack reservation
per task, fatal exhaustion unless another design is chosen, suspension effects
that must still be checked, and a scheduler-aware I/O boundary. Full Go-style
growable-stack M:N execution is a much larger commitment. The
[runtime choices](stackful-tasks/runtime.md) separate these options.

## What to borrow from Go

Go starts ordinary function calls concurrently, and its channels provide typed
communication. Arguments are evaluated in the starting goroutine. The call's
return values are discarded by a `go` statement. These are language semantics,
not a promise about which operating-system thread runs the function.
[Source: Go specification](https://go.dev/ref/spec#Go_statements).

For Plenty, retain ordinary function bodies and explicit communication, while
using a fallible library operation that returns a scoped result handle. Spawning
must preserve ownership on failure. Starting a task cannot silently succeed when
there is no stack or queue capacity; detached tasks would also conflict with the
initial lifetime/cleanup contract. No `go expression`, unbounded scheduler queue,
automatic GC, implicit channel allocation or process-wide hidden executor follows
from this proposal.

Go's scheduler distributes goroutines over worker threads, with separate
goroutine, worker and processor state. Its runtime includes parking, syscall
transitions and work distribution. That implementation is evidence of the work
behind the surface, not a minimum dependency for a single-worker experiment.
[Source: runtime scheduler](https://go.dev/src/runtime/proc.go).

## Candidate surface

Prefer library APIs and familiar `with`, explicit captures, `Result`/`?`, concrete
jobs and consuming result handles. This sketch is **proposed API spelling**;
neither `std.task` nor these error types exist:

```text
import std.task as task

def answer(value: i64) -> Result[i64, task.Cancelled]:
    task.checkpoint()?
    Ok(value + 1)

def compute() -> Result[i64, task.ScopeError | task.StartError[()] | task.Cancelled]:
    with task.scope(max_tasks=8, stack_bytes=65536)? as scope:
        with scope.spawn(def once () -> Result[i64, task.Cancelled]: answer(41)).drop_job()? as work:
            return Ok(work.join()?)
```

The sketch omits typed channel construction; it is not a proposed rewrite of the
existing `std.thread` or `std.sync` APIs. Names and whether scope/handle contexts
should be nested remain open. Keep task types distinct until their ownership and
wait behavior actually satisfy the native-thread contracts.

The parent evaluates captures and arguments first. `scope.spawn(job)` attempts
admission, commits ownership only on success and schedules the body for later
execution. Failure returns the unstarted concrete job, following
[consuming native jobs](../design/facilities/03-concurrency/jobs.md).
An allocation-free `.drop_job()` conversion would discard that job deliberately.
A scope reserves its task table and ready/wait bookkeeping fallibly. Capacity
exhaustion is a separate `Full(job)` result; `spawn_nowait` is the initial policy.
Waiting for capacity while holding a dependency can deadlock, so blocking
admission needs an explicit later contract. A completed slot may be reused only
after its result/handle lifetime is resolved; the spike does not implement reuse.

Channels have explicit positive capacity, fallible construction and owned
messages. A successful send commits ownership exactly once. Full/nonblocking,
closed and cancelled send errors return the unsent value without allocating;
a parked send retains its value in its task until it commits or returns it.
Wait nodes are reserved with admission, and park/wake/cancel never grow a queue.
Dropping the last sender wakes receivers to observe closure; closing the
receiver side wakes senders with their values. Cancellation is checked before a
new transfer commits; an already committed transfer is not rolled back. Returning
a cancelled unsent value must still respect linear obligations. Rendezvous and
general selection are outside the first candidate. These are proposed contracts,
not behaviors proved by the integer-only spike.

`join()` drives or yields to the owning scheduler until completion, then moves
the result once. Returning a `Result` is ordinary task output, not a request to
cancel siblings. Scope teardown joins all admitted children on every normal exit,
including `?`; unclaimed results are dropped once. Parent errors cannot erase
child ownership or silently discard unresolved linear obligations. Existing
[linear integration](linear-integration.md) remains a separate proposed contract.

## Loans, suspension and migration

An ordinary stack remains at the same virtual address throughout task execution.
Task-local loans and loans down its synchronous call chain may survive a yield
without becoming movable self-referential frame fields. No public pin wrapper is
needed. The runtime still promises address stability internally; hiding the
promise does not remove its implementation obligations.

Initially accept wholly owned concrete jobs/results and prohibit references in
messages, escaping callbacks and stored task handles. Within one task, preserve
ordinary lifetime, exclusive-loan, owner-move and container-invalidation checks.
Interleaving on one OS thread never authorizes two exclusive loans. Adding parent
loans later should reuse scoped-worker reasoning: join before the owner dies,
prevent conflicting parent accesses throughout the scope, and reject cycles.
Do not infer those rules from the C experiment, which has no borrow checker.

Start with one scheduler bound to one native thread. Cross-thread task creation,
handle transfer and migration are excluded. Owned inputs must still satisfy a
defined task effect/eligibility check; existing native-worker certification does
not prove that an operation is safe to suspend. A future M:N mode needs
structural thread eligibility for captures, every live local, references,
callbacks and destructors, and a contract for thread-local state and foreign
handles. Merely making the initial closure transferable is insufficient.

Ordinary syntax can hide significant suspension. Infer a separate transitive
`may suspend` effect for task operations and their callers; validate scheduler
context at every reachable entry. A task-only helper must be rejected from an
ordinary non-task entry. Calls through stored callbacks require evidence too.
Reject suspension while a mutex guard, an uncertified foreign call, or a
destructor is active. A pure helper can be called in either context. These checks
may be a substantial compiler addition even without `async`/`await` keywords.

## Cancellation and deterministic cleanup

Cancellation is a request, observed at task-aware waits and explicit checkpoints.
It wakes parked channel/I/O waits and returns a typed cancellation error. The
function then returns through ordinary cleanup and `?` paths. Cleanup runs
synchronously on that task's retained stack before join releases the stack.
Destructors cannot suspend, recursively drive the scheduler or allocate hidden
task storage. Explicit fallible finishing operations handle protocol shutdown
before ordinary destruction. A linear obligation cannot be abandoned merely
because cancellation was requested.

Do not free a suspended live stack, asynchronously throw into arbitrary native
code, use `longjmp` across destructors, or treat signal-handler unwinding as safe
cancellation. A running task may ignore cancellation; scope exit can consequently
wait forever. Fatal traps retain Plenty's existing no-unwind behavior.
Synchronous destruction also does not imply nonblocking destruction: a blocking
close/drop path can stall this worker. Real adapters need that audit.

Go's pipeline guidance explicitly teaches cancellation when downstream consumers
stop early; convenient spawning and channels do not arrange application lifetime
automatically. Plenty should make the scope boundary explicit and preserve its
existing endpoint/join deadlock checks. Those checks are not a deadlock proof.
[Source: Go pipelines and cancellation](https://go.dev/blog/pipelines).

## Comparison with the async exploration

The independent [async/await exploration](async-await.md) describes restricted
movable frames, with its [ownership contract](async-await/ownership.md) and
[native spike evidence](async-await/evidence.md). Both experiments are available
for comparison; neither adds a production feature or depends on the other's
implementation.

| Question | Restricted movable async frames in that exploration | Fixed-stack task candidate here |
| --- | --- | --- |
| Suspension syntax | Explicit `await` inside async functions | Ordinary calls; inferred suspension effects |
| References across suspension | Initially excluded, including hidden child borrows | Task-local call-chain borrows can remain at stable addresses |
| Storage | Concrete frames inline; explicit fallible root admission | Explicit fallible task record and a whole stack reservation |
| Destruction | Synchronous cleanup of initialized frame fields | Ordinary synchronous stack cleanup after cooperative return |
| Cancellation | Pending operation must support safe abandonment | Task must resume, observe cancellation and return |
| Existing blocking functions | Need an owned nonblocking adapter or blocking pool | Also stall the worker unless adapted or offloaded |
| Runtime portability | State machines can use ordinary native calling conventions | Stack switching requires ABI/tooling/platform integration |

Both candidates avoid exposing pinning by narrowing their contracts. Neither
establishes general async-drop, cancellation latency, linear resolution, real
I/O suitability or a concurrency feature ready for adoption. The async spike's
small frame layouts and this spike's fixed stacks are observations of different
workloads, not a fair performance comparison. A full Go emulation is not an
established simpler alternative to either restricted candidate.

## Decision still open

The experiment makes fixed-stack tasks worth comparing with the async candidate,
especially when borrowed-buffer helper chains matter. Adoption needs the same
bounded TCP/HTTP workload, cancellation and failed-start tests, a realistic stack
budget, I/O registration-lifetime checks and compiler effect/loan experiments.
Measure throughput, p95 latency, committed/resident/virtual memory, admission
failure and cleanup work under equal limits. Those acceptance gates are recorded
only in the [backlog](../backlog.md). Keeping native workers, adopting a narrow
task library, choosing restricted async, or adopting neither all remain possible.

# Actor budgets and retained stacks

Status: **Exploratory; no production scheduling change.** A test-only native
probe validates stack retention with the existing actor heap router and trap
recovery. It does not insert compiler checkpoints or drive `std.actor.Stage`.
The [actor reference](../design/stdlib/actor.md) still specifies handlers that
run to completion. Remaining implementation and qualification scope belongs to
B85 in the [backlog](../backlog.md). See the [probe evidence](actor-budget-scheduling/evidence.md).

## Question and provisional decision

Could compiler-inserted budget checks let an actor yield inside ordinary nested
calls, without changing application methods into generators?

The mechanics are plausible with immovable, guarded stacks on one scheduler
thread. A checkpoint decrements the active actor's approximate work budget. On
exhaustion it saves the actor's execution and runtime state, switches to the
scheduler, and later continues after the check. Helper calls keep their normal
calling convention. The runtime must separately preserve state currently stored
per native thread, and the stage must stop lending the same exclusive `Context`
to overlapping handler activations. These are substantive integration changes.

This is a narrower investigation than [general stackful tasks](stackful-tasks.md):
actor state already has an isolation boundary, mailboxes remain bounded, and a
suspended callback remains the actor's only active callback. The older C task
spike proves nested stack retention; the new Rust probe exercises actual actor
trap recovery and allocation routing. Closed local PRs #639/#642 explored
socket-parking fibers before the implemented actor API; they did not validate
budget checks or the current stage's ownership contract. No unmerged source
from those PRs is used here.

## Scheduling and compiler boundary

Checks belong on executed loop cycles and recursive call cycles, including tail
calls. A check only at the outer actor entry misses an unbounded helper loop.
Calls through callbacks and generators also need coverage; inlining, loop
specialization, SIMD lowering and cached object reuse must preserve that
coverage. Existing lowering has distinct loop, tail-call and callback paths, so
adding one check to one loop emitter would not establish fairness.

An initial budget can count approximate work between checks. It is not a time
limit: a single large operation may have substantial cost. Long runtime
operations need chunking, their own safe checkpoints or worker offload. Measure
both uncontended check overhead and peer progress under expensive operations
before choosing a charging policy. The probe's quantum of 17 is only a
deterministic test parameter.

One possible implementation shares compiled helpers and makes a checkpoint a
no-op outside an actor, while honoring an explicit non-suspending region inside
one. Actor-specialized helpers could reduce unrelated overhead but increase code
and cache variants. Neither option has been implemented or measured. Every
checkpoint must be visible to code generation as an ordinary call with the
appropriate memory effects; switching stacks does not relax aliasing rules.

Do not switch inside allocation, heap-to-heap message copying, stage queue
mutation, foreign-library arenas, or synchronous destruction. Compiled code
invoked by a runtime helper can reach another checkpoint: excluding only Rust
function bodies is insufficient. Transitive compiler effects or a runtime
non-suspending depth, with audited entry/exit and trap restoration, must protect
these regions. Deferral can itself be unbounded unless the protected operation
has a bounded cost or an offload path.

## Actor and scheduler ownership

Today `Stage.deliver` passes `&mut self._nodes[index]` and
`&mut self._context` to `__actor_run`. Its adapter arguments and result storage
are slots in the caller's native stack. `Context` owns the shared mailboxes,
ready queue, selector, timer table, and current actor/spawn/stop fields.

A suspended callback cannot retain that exclusive context while the scheduler
and another callback access it. Saving `_self` alone would repair the visible
identity but leave overlapping exclusive loans. A candidate replacement is one
context capability per activation, holding actor-local identity and pending
commands, with stage access restricted to short non-suspending operations. Its
interior access needs an explicit compiler/runtime ownership contract. A raw
pointer hidden in the library is not sufficient proof of soundness.

Node slots, callback arguments/results, and context storage must stay at stable
addresses until the callback finishes or traps. Scheduler dispatch frames must
return independently of suspended actors. Keep suspended callbacks runnable
even with empty mailboxes; enqueue new mail without starting another callback
for that actor. Run spawn hooks, settlement and restart policy at their defined
lifecycle points. Do not restart an actor or destroy its stack while a suspended
activation still references its state.

## Runtime, failure and resources

The probe places `plenty_actor_run` and its recovery frame on the actor's own
stack. This lets the existing nonlocal trap path return within that actor after
several suspensions. The scheduler's stack is separate. Each transition restores
the intended allocation heap, recovery pointer, caller source location, drop
queue and last-trap state. Arena calls and active destruction are excluded from
probe checkpoints. Other TLS, callback, lock and FFI boundaries still need an
audit; the demonstrated snapshot is not an exhaustive runtime contract.

The actor owning the stack also owns the trap boundary. Normal completion and
cooperative cancellation return through ordinary cleanup; a contained trap
abandons frames without destructors, following existing actor semantics, then
discards the heap. Resuming a trapped or completed context is forbidden.
Scheduler deadlines and cancellation can be examined between quanta, but
delivering `Mail.Stop` does not interrupt a callback already in progress.
Cancellation, stage teardown and any forced-discard policy need a separate
defined contract; stack switching does not make arbitrary stack abandonment safe.

The experiment uses fixed stacks with inaccessible guards. Stack admission must
be fallible and bounded in a production implementation, with a separately
visible stack-memory cost. Virtual reservation is not resident usage or a
guarantee of later physical memory. Guard pages do not prove that oversized
frames cannot skip guards. Existing stack-overflow reporting assumes the native
stack and its alternate-signal-stack reservation; alternate stacks require
explicit bounds and signal-handler qualification. Overflow remains process-fatal
under the current contract. Recoverable stack overflow is not established here.

The probe uses glibc `swapcontext`, which preserves more state than a minimal
ABI switch and may involve a system call for signal masks. It is a correctness
vehicle, not the selected production switch. A custom switch needs callee-saved
register and floating-point/control-state tests, stack alignment, platform
hardening, debugger/unwinder and sanitizer integration. The experiment qualifies
only its host x86-64 GNU/Linux configuration; AArch64 and other targets are not
covered. Thread migration is outside this candidate.

Blocking native operations still require nonblocking actor adapters or
[bounded blocking-work offload](actor-blocking-work.md). A checkpoint cannot
interrupt a blocked syscall or arbitrary native computation. Where possible,
compiler checks should reject known blocking operations from actor-reachable
code, including indirect calls, with explicitly audited offload as the escape.

## Assessment

Retained stacks avoid a generator transformation throughout the call graph and
support actor-local borrows across nested suspension. The actual heap router and
trap boundary can coexist with suspension when their state is restored. This is
enough to justify a compiler/stage integration prototype, without adopting a
production scheduler or claiming BEAM-equivalent fairness. The ownership of the
shared stage context is the largest architectural question exposed by this
probe. The backlog records the remaining gates.

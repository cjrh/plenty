# Actor recovery and safety qualification

Companion to [Stronger safety boundaries](../trusted-safety-boundaries.md).
Status: **Proposed; not implemented.** The implemented
[actor trap contract](../../design/stdlib/actor.md#traps) remains authoritative;
this page proposes additional certification and acceptance criteria, not a
finding that an existing path violates memory safety.

## Certify abandonment separately

Actor containment abandons handler frames without destructors, frees the actor
heap, and continues the stage. The essential property is that surviving state
remains valid after that abandonment. Thread safety, allocation freedom, and
ordinary borrow correctness do not independently establish this property.

Extend effect analysis with an abandonment contract, distinct from
[`@thread_safe`](../../design/facilities/04-interop/trusted-declarations.md#worker-threads).
Certification must account for reachable helpers, callbacks, generators,
destructors, native operations, and the storage that survives the boundary.
An absent summary is insufficient evidence. Native summaries are explicit
trusted promises, never inferred from a function name or its C signature.

The compiler/runtime boundary must establish:

- Surviving nodes, stage tables, queues, callback registrations, native objects,
  and runtime caches retain no usable pointer into a discarded heap.
- Partially updated state outside the heap remains valid without abandoned
  cleanup. A lock staying held may cause deadlock; skipped cleanup must never
  expose invalid memory or make a later release unsafe.
- Every retained resource has an abandonment policy: close or invalidate it,
  intentionally leak it, or forbid containment while it is active. Leaks must
  be documented separately from memory corruption.
- Recovery restores the correct heap, trap boundary, allocator routing, and
  other runtime context before scheduling another actor.

Current node restrictions, message copying, and the prohibition on starting a
worker inside a handler are useful mechanisms to preserve. Stronger checking
must cover values reached through calls and native state as well as values
stored directly in the node.

## Native callback crossings

Consider an actor calling C, which calls an exported Plenty callback, which
traps. Jumping directly to the actor scheduler would abandon C's frames too.
C might hold a lock, have partially updated an object outside the actor heap,
or require cleanup before its caller can safely proceed.

Use a conservative native-call boundary: suspend actor containment while an
unqualified foreign call is active. Re-entering Plenty through a callback does
not restore containment automatically. A trap in that callback terminates the
process under the ordinary fatal-error policy. This requires a per-thread,
nesting-aware guard, not only a check of the innermost Plenty function.

A later native abandonment contract may admit a specific crossing when the
binding establishes that all intervening frames and surviving resources can
be abandoned safely. It must also account for native retention and allocator
callbacks. Marking a function thread-safe or asserting that it never unwinds
does not authorize jumping over its frames.

Diagnostics or trust reports should identify operations that suppress
containment. Preserve typed native errors on normal return. Do not attempt to
resume an abandoned native call or return a fabricated success to it.

## Relationship to other actor designs

The [actor budget proposal](../actor-budget-scheduling.md) also requires safe
suspension regions and correct restoration of shared execution context. A
region safe to suspend is not automatically safe to abandon permanently.
Track those properties separately, while sharing effect provenance and
entry/exit bookkeeping where their contracts coincide.

[Blocking-work offload](../actor-blocking-work.md) isolates native workers
from actor heaps. Its ownership and completion contracts must remain intact;
recovering an actor does not authorize abandoning a worker's native frames.
Internal Rust panics and stack overflow remain process-fatal under the current
actor contract. This proposal does not extend recoverability to them.

## Acceptance criteria

These are adoption gates, not tests claimed to have run for this proposal.
Implementation scheduling remains in B98 in the [backlog](../../backlog.md).

| Area | Required evidence |
| --- | --- |
| Trusted confinement | Reject unchecked access from ordinary modules through direct calls, re-exports, generics, and function values; accept checked wrappers |
| Foreign owners | Reject duplicate ownership, release during a loan, and use after transfer; exercise failed acquisition and consume-on-success failure without double release |
| Native retention | Exercise callback registration/unregistration, library residency, and attempted loans escaping their owner; raw escapes remain explicitly trusted |
| Actor isolation | Repeated contained traps with allocation-failure injection, messages, parent/child construction, and external state; surviving values remain valid and discarded resources cannot be reused |
| Callback crossings | C-to-Plenty and nested Plenty-to-C-to-Plenty callbacks; traps across unqualified native frames are process-fatal in subprocess tests; normal returns restore containment correctly |
| Context restoration | Nested boundaries, ordinary errors, destructors, and subsequent actor execution restore heap and recovery context on each implemented target |
| Callback robustness | Inconsistent comparators, changing size estimates and views, malformed ranges, and failing keys preserve bounds, initialization, valid text, and one owner per element |
| Trust policy | Transitive and renamed dependencies, generated bindings, native link inputs, and cached builds cannot bypass application admission; reports have stable identities and explanatory paths |
| Process state | Concurrent reads/spawns and worker-triggered fatal exits respect documented environment, signal-stack, output, and native cleanup contracts |

Use existing allocation accounting, ownership/drop counters, subprocess probes,
and applicable native sanitizers to test these properties. Deterministic
hostile callbacks and fault injection should complement fuzzing. Native
platform execution is required for ABI, recovery, and shutdown claims;
cross-compilation alone is insufficient.

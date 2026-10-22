# Stack, scheduler and allocation choices

Companion to the [Go-style task exploration](../stackful-tasks.md).
All task designs here are candidates; today's
[native workers](../../design/facilities/03-concurrency/threads.md) remain the
implemented contract.

## Four different implementation commitments

| Model | Simplification | Cost and boundary |
| --- | --- | --- |
| Existing native workers/executors | OS manages stacks/scheduling; ordinary synchronous code and checked ownership | Each running worker uses a native thread; executor queues jobs, rather than suspending arbitrary call stacks |
| Fixed-stack cooperative tasks on one worker | Stable stack addresses; no relocation maps; explicit bounded scheduling | Per-task stack budget, no automatic preemption, task-aware blocking, fatal unchecked overflow |
| Fixed-stack tasks over several workers | Potential task concurrency and parallelism without stack copying | Cross-thread eligibility/migration, synchronization, wake races, TLS/FFI, work distribution, shutdown |
| Growable-stack M:N goroutines | Small initial stacks and scalable ordinary call chains | Compiler stack checks, relocation/segmentation, failure semantics, syscall handoff, preemption, pointer maps and tooling |

The one-worker experiment is stackful **M:1** execution. It is not an M:N
scheduler or a native-thread pool. Its stack switching shows that ordinary C
calls can suspend below their callers, without manually translating every call
into an async state machine. It demonstrates no million-task scalability,
parallelism or end-to-end I/O speedup.

## Stack choices and recoverable allocation

Go's runtime checks stack bounds in generated code, allocates replacement stack
storage, copies stacks and adjusts pointers. Its stack allocator has fatal OOM
paths. Go's stack machinery is integrated with its pointer metadata and runtime;
removing GC does not supply the relocation machinery for free.
[Source: runtime stack implementation](https://go.dev/src/runtime/stack.go).

For Plenty, assess these independently:

- **Fixed guarded stacks:** choose size and maximum task count explicitly.
  Allocate task records and mapped stacks before transferring the job; failed
  reservation is an admission error. This avoids later stack-growth allocation
  and keeps interior addresses stable. It wastes address space and commits pages
  as touched; a guard trap still terminates unless a separate safe failure path
  exists. Large stack frames can skip a guard without stack probes.
- **Reserved, incrementally committed stacks:** stable virtual addresses can
  reduce initial resident cost, but later commitment can fail. Reserving address
  space or successful `mmap` does not guarantee physical memory on Linux with
  overcommit. Signal-driven commitment does not automatically yield a typed,
  recoverable allocation error or safe cleanup. Platforms also have different
  reservation/commit contracts.
- **Movable contiguous stacks:** compiler-generated pointer maps and provenance
  rules must account for interior pointers, registers, channel wait records,
  callbacks and foreign pointers. Stable heap allocation alone does not repair
  stack pointers. Such metadata can exist without tracing GC, but writing it is
  new compiler/runtime work. Restricting every borrow across every possible
  growth point largely sacrifices the benefit over restricted async.
- **Segmented stacks:** old addresses can stay stable, but calls/returns need an
  alternate ABI, segment metadata, foreign-call bridges and reclamation rules.
  Segment growth still allocates inside an ordinary call chain.

A recoverable stack-growth failure cannot simply appear as `AllocError` in an
arbitrary function declared `-> i64`. Choices are to propagate a new effect and
error through all possible growth points, reserve a compiler-proven complete
budget, restrict call graphs/recursion, or retain fatal exhaustion. Each changes
the source contract or implementation substantially. This proposal provisionally
uses **fallible explicit admission with fatal unchecked stack exhaustion**,
matching today's native-worker exhaustion boundary. If the requirement is that
all later stack capacity failures must be recoverable, this candidate is
incomplete and should not be adopted on the present evidence.

## Allocation ledger

Fallible allocation means all explicit recoverable resource requests are
reported; it is not a promise that OS process termination can be recovered.
The candidate must advertise the fixed-stack limitation directly.

| Request | Failure ownership and recovery |
| --- | --- |
| Scope task table, ready queue and wait records | Fallible construction; no live children on failed construction; preallocate bounded bookkeeping |
| Task record, stack reservation and native context setup | Return complete unstarted job; unwind partial setup before admission; no body execution |
| Slot capacity exhausted | Return `Full(job)` without allocating, distinct from resource failure |
| Bounded channel buffer and readiness registrations | Explicit fallible construction/registration, rollback on failure; no hidden allocation on park/wake/cancel |
| Task body allocation | Ordinary typed `Result`; return through normal initialized-resource cleanup |
| Result storage | Reserve with admission or keep result in task cell; preserve concrete size/type and exactly-once transfer/drop |
| Optional blocking worker submission | Explicit bounded fallible handoff, preserving job/inputs on failure |
| Fixed stack exhausted / OS kills process under memory pressure | Fatal boundary; no claim of a returned job error or reliable cleanup |

The spike accounts for queue, task record, stack mapping and body allocations.
It does not implement real readiness registration, result objects, OS worker
creation or operating-system physical-memory admission guarantees.

## I/O, foreign calls and fairness

Today's [`std.ready`](../../design/stdlib/ready.md) waits use a bounded native
poll; today's channel/thread/executor waits block native threads. Running these
unchanged inside one cooperative worker would block every task on that worker.
The native operations are not automatically task-aware because ordinary syntax
looks similar.

A candidate task backend needs nonblocking handle operations, owned readiness
registrations and cancellation that removes those registrations before the
buffer/handle can drop. Borrowed stack buffers can remain stable while parked,
but late completion must never access them after return or cancellation. A
kernel operation that cannot be synchronously cancelled needs a separate owned
operation/result lifetime or explicit completion; stackful syntax does not solve
that problem. Partial read/write progress and typed transport errors must survive.

Initially reject blocking OS/FFI calls in task bodies through effect checks, or
provide an explicit bounded native-worker offload with owned transferable data.
An offload must be joined/completed before freeing captured resources; requesting
cancellation does not terminate its foreign call. DNS, file I/O, TLS and user
destructors need separate audits. Native thread TLS, `errno`, signal masks,
stack bounds and thread-affine native handles need defined treatment before any
migration; a scheduler cannot assume that saving CPU registers solves them.

Cooperative scheduling offers no preemption of CPU loops, arbitrary FFI or
destructors. Task-aware channel waits and explicit checkpoints provide known
suspension points; inference makes their reachability visible to the compiler.
Document fairness and bounded work per turn. A later preemptive scheduler must
define safe points, stack scanning and lock/runtime critical sections; forced
cancellation still must not interrupt cleanup. Debuggers, profiler unwinding,
sanitizers, guard pages, ABI register state and supported targets also need
qualification. Linux `ucontext` is only an experiment backend, not a portability
plan.

## GC-free lifetime and scope exit

Stack and task records have one owning scope; channels transfer owned values;
reference-counted shared owners follow today's cycle restrictions. Ready/wait
queues contain non-owning task identities whose validity is protected by the
scope. No task survives scope completion. Drop order is explicit: request
optional cancellation, resume children until ordinary return, transfer/drop
results and captures, detach wait records, then reclaim task stacks and the scope.
Do not destroy a scope while its scheduler context is still referenced.

A child waiting forever or ignoring cancellation prevents scope exit. The
runtime can detect some states with no ready task and no external registrations,
but cannot prove general deadlock freedom or safely free live stacks as a
fallback. Joining parents/children and bounded channel dependencies retain the
existing analysis problems. Smaller surface syntax changes none of these
lifetime facts.

# Concurrency and parallelism

Concurrency is explicit. Native workers can overlap without an interpreter lock;
ordinary loops and comprehensions remain serial. Ownership transfers and checked
loans still apply across threads. Shared immutable storage does not permit
unsynchronized mutation; [`std.atomic`](../../stdlib/atomic.md) values are
updated through shared references with atomic instructions, and a
[mutex](mutex.md) grants one lock holder at a time exclusive access to its value.
A [`Shared[T]`](../01-builtins/shared.md) handle gives several owners, such as
pool jobs, shared access to one value; the last owner, on any thread, drops it.

The [Go-style scoped-task exploration](../../../proposals/stackful-tasks.md)
assesses a possible lightweight alternative, including a fixed-stack native
experiment. It introduces no implemented task API or change to these contracts.

Worker eligibility is a closed-world compiler check, not a user-defined marker
protocol. It examines concrete stored types, reachable helper and callback
bodies, captures, results, and destructors, including finite recursive type
graphs. Scalars, strings, ranges, collections, classes, enums, and `Shared`
handles are eligible when their contents and effects are eligible. A shared
value's destructor is checked as if any holder's thread could run it. Stored concrete callbacks retain
their eligibility evidence.

Each native operation has a declared [effect](effects.md). Files, directory
streams and child pipes cross threads under the ordinary ownership and loan
rules; changing the working directory is rejected. Foreign pointers/calls,
uncertified indirect callable effects, and generator frames are conservatively
excluded. Imports do not grant thread permission. Ordinary checked
standard-stream I/O is usable, with scheduler-dependent ordering.
Owned transferred values cannot contain references or borrowed environments.

Scoped threads can borrow parent storage for the task's lexical scope. Channels
and executor jobs require wholly owned storable values. Executors and futures
cannot enter worker code, including through containers or messages: worker-side
submission and future waiting are unsupported.

- [Scoped threads](threads.md) and [consuming jobs](jobs.md).
- [Bounded channels](channels.md), including the
  [recommended patterns](channels.md#recommended-patterns) that keep joins,
  result waits, and pool shutdowns from waiting on a live endpoint.
- [Executors and shutdown](executors.md), [futures](futures.md), and
  [ordered mapping](parallel-map.md) / [fixed-tree reduction](parallel-reduce.md).
- [Mutexes](mutex.md).
- [Cancellation tokens](cancellation.md) and [timed waits and selection](timed-selection.md).
- [Native effects on worker threads](effects.md) and deadline-bounded I/O
  through [`std.ready`](../../stdlib/ready.md).

Allocation guarantees exclude argument expressions, worker bodies, and user
cleanup. Error wrappers add no allocation. Blocking operations offer no strict
waiter fairness, and application communication can deadlock. Normal scope exits
join workers; fatal traps terminate without unwinding. A nonterminating worker
prevents its scope or executor from finishing. Worker stack exhaustion terminates
the process rather than becoming a job error.

# Thread-local reference counts

> **Status: Not implemented; stage 1 measured and rejected.** Design note
> for issue #746. Stage 1 was built (issue #751, local PR #944) and removed
> every locked count instruction from k-nucleotide without a measurable cycle
> gain, so the PR was closed and stage 2 is not recommended; see
> [measured result](#measured-result). The
> [memory and destruction](../design/toolchain/02-architecture/memory.md) and
> [concurrent runtime](../design/toolchain/02-architecture/concurrency.md)
> pages own the current contract. Issue #744 (no count pair around borrowed
> arguments) and issue #745 (sole-owner release) have landed.

## Summary

Every runtime count update is a `lock`-prefixed read-modify-write, although
most objects never leave their thread. This note proposed the Lean 4 and Koka
design, a per-object thread-shared mark set at certified thread boundaries,
starting with a process-wide single-threaded phase that needs no marking.

Stage 1 was built and removed every locked count instruction from
k-nucleotide, with no measurable change in cycles. On the measured processor
the locked instructions are not the cost, so neither stage is recommended.
The design below is kept as the record of what was considered.

The note also found that the retain half of issue #745's fast path is unsound
without a thread-locality guarantee; see
[the sole-owner fast path](#the-sole-owner-fast-path).

## Measured result

Stage 1 was implemented as described below (issue #751, local PR #944): a
live-thread count raised only in `plenty_thread_start` and lowered only after
`plenty_thread_join` returns, with plain count updates while it is zero and a
`runtime-checks` trap for count updates from unregistered threads. Measured
against `main` at `d7a70ede` (issues #744 and #745 landed) on an AMD Ryzen 7
7840HS (Zen 4), with callgrind and `perf stat -e cycles:u` pinned to one core:

| Workload | Locked count instructions | Instructions | Cycles |
| --- | --- | --- | --- |
| k-nucleotide (fasta 50,000) | 1,399,543 to 0 | +0.2% | Within noise (under 2%) |
| reverse-complement | About 0 before and after | Unchanged | Within noise |
| Parallel k-nucleotide (7 pool jobs) | Unchanged: workers are live | +0.4% | Within noise |
| Retain/release loop, 50M iterations | All removed | About 3 more per operation | Unchanged |

In the loop benchmark, `plenty_retain` and `plenty_release` fell from 46% to
7% of cycle samples while total cycles stayed the same.

The premise of this note was wrong for this processor. Issue #743 attributed
k-nucleotide's 26.5% of cycles in `plenty_release` to the `lock decq`, because
96% of that function's samples fell on the instruction after it. An
uncontended locked operation on a cache line the thread already owns is cheap
on Zen 4; the samples land after the first access to the object's header,
which is the likely real cost, attributed to the next instruction. This has
not been confirmed with cache-miss counters, and processors with costlier
atomics (some AArch64 cores, older x86) were not measured.

Consequences:

- Stage 1 adds about three instructions per count operation for no measured
  gain, so it was not merged.
- Stage 2 removes the same locked instructions on parallel programs and adds
  marking walks; on this evidence it is not worth building.
- The cost to attack is touching object headers: fewer count operations
  (issue #755, a last-use move after a borrow), fewer separately allocated
  objects, and confirming header misses with cache counters before further
  work on count atomicity.

## Why counts exist and why they are atomic

Plenty has single ownership at source level, but runtime objects begin with
`{AtomicU64 refs, destroy}` (`plenty-runtime/src/memory.rs`). A count above
one arises from:

- copying an immutable heap `str`, or an inline value containing one, which
  retains instead of copying;
- [`Shared[T]`](../design/facilities/01-builtins/shared.md) handles, whose
  `share()` retains one box;
- native helpers that return retained projections and builder aliases, and
  the retain/release pairs still emitted around some borrowed arguments and
  receivers (issues #744, #741).

Strings of up to seven bytes are inline and own nothing; their count calls
return before touching memory. Literals use the immortal count `u64::MAX`.

Counts are atomic because counted owners can cross certified boundaries, and
because a worker can retain through a borrow: a scoped worker that copies a
`str` out of a borrowed `list[str]` increments a count the parent may be
incrementing at the same time. A release site cannot tell whether that ever
happened to its object.

Release uses `fetch_sub(Release)` and an acquire fence on the final count;
retain uses a relaxed compare-and-swap loop that rejects overflow. After the
final release the count word becomes the destruction queue's link
(`plenty_release`, `continue_after`), on the destroying thread only.

## Where an object can reach another thread

| Boundary | What crosses | Concurrent count access? |
| --- | --- | --- |
| Scoped `spawn` with `&T` arguments or `spawn(&job)` captures | Borrowed parent graph | Yes: worker and parent both retain |
| Scoped `spawn` with moved, copied or `&mut T` arguments | Owned or exclusively borrowed graph | Yes: a `str` inside can share an object with a parent local |
| Scoped join result | Worker result | No: the worker's frame is gone and join orders it |
| `pool.submit` and consuming `spawn` | Job environment | Yes, as for moved arguments |
| Executor map's shared environment | One environment borrowed by every job | Yes |
| Future result | Job result | No: the adapter finishes, including environment cleanup, before `Phase::Ready` is published under the job lock (`executors.rs`) |
| Channel send | Message | Yes |
| `.par` pipelines, ordered map, fixed-tree reduce | Borrowed list and callback captures | Yes |
| `Shared[T]` construction | Contents, and the box itself | Yes: every holder retains the box and copies from the contents |
| Store under a `Mutex` lock | The stored value | Yes, when the mutex is reachable from another thread |
| `std.actor` | Messages are copied into the receiver's heap; all actors run on one thread | No |
| FFI | `ForeignPtr` cannot enter workers; [exports](../design/facilities/04-interop/exports.md) require one caller thread | No |
| Globals | None exist; constants are immortal | No |
| Runtime-internal threads | The detached DNS lookup thread in `aggregates/net.rs` holds only Rust data | No |

Every Plenty thread starts in `threads::plenty_thread_start` (scoped tasks and
executor workers alike). `threading::check` in `src/threading.rs` already
walks each crossing type for eligibility, with `src/effects.rs` classifying
the native operations a worker can reach; both are compile-time type walks.
Marking adds a runtime walk over values of the same types.

## Prior art

| System | Mechanism | Fit for Plenty |
| --- | --- | --- |
| Lean 4 | `m_rc > 0` single-threaded, `< 0` multi-threaded, `0` persistent. `lean_dec_ref` inlines the `rc > 1` case; `lean_dec_ref_cold` does the atomic `fetch_add` on negative counts. `lean_mark_mt` walks the graph with a worklist and negates counts; `alloc_task` calls it on a task's closure, and task results are marked when resolved. Marks are never removed. Lean copies a shared object instead of updating it in place, so a marked object never points to an unmarked one. | Good. Boundaries are explicit and compiler-known. Plenty mutates in place, so it needs a store barrier where Lean copies. |
| Koka (Perceus) | Follows Lean: thread-shared objects have negative counts, so one inlined `rc <= 1` test takes the slow path for both "shared" and "last reference". `tshare` marks recursively; starting a thread marks its argument; storing into a thread-shared mutable reference is the other sharing operation. | Same as Lean; Koka's mutable-reference rule corresponds to Plenty's `Mutex` stores. |
| Biased RC (Choi, Shull, Torrellas, PACT 2018); CPython 3.13 free-threaded (PEP 703) | Each object has an owner thread with a plain local count; other threads use an atomic shared count. A negative shared count queues the object for the owner to merge. Swift prototype: RC operations over twice as fast, client programs 22.5% faster on average. CPython uses separate `ob_tid`, `ob_ref_local` and `ob_ref_shared` fields, immortal objects and deferred counting. | Poor. Needs an owner id per object and a thread-id comparison per operation; the 16-byte header has no room, and owner-thread merge queues conflict with the destruction queue's reuse of the count word and with pool threads that finish. Its advantage, no walk at a boundary, matters less when the compiler knows every boundary. |
| Swift | Atomic counts on every class reference. OSSA `copy_value`/`destroy_value` and copy propagation delete redundant pairs. `-assume-single-threaded` makes all reference-counting instructions nonatomic, unchecked. Ungar, Grove and Franke (DLS 2017) report ARC taking up to 80% of time and propose dynamic atomicity: a store barrier plus generated tracers that mark objects reachable from shared storage. | Plenty's current model. Pair elimination is issue #744. The phase flag below is a checked, dynamic `-assume-single-threaded`. Dynamic atomicity is the per-object design with a store barrier. |
| Rust | `Rc` is non-atomic and not `Send`; `Arc` is atomic. Owned values have no count. | Would split `str` into local and sendable forms, a user-visible distinction the language avoids. `Shared[T]` already plays `Arc`'s role. |
| Nim ARC/ORC | Counts are never atomic; `isolate` checks at compile time that a subgraph has no outside references, and `Isolated[T]` moves it between threads. `--mm:atomicArc` is the thread-safe variant. | Moved Plenty values are not isolated, because `str` copies share storage. Copying strings at each boundary would add hidden allocation, and `Shared[T]` contents are read by several threads anyway. |

## Recommendation

### Stage 1: single-threaded phase

Keep a process-wide count of live Plenty threads other than the main one.
`plenty_thread_start` increments it before `pthread_create`; the parent
decrements it after the corresponding join or pool shutdown. While it is
zero, retain and release use plain (relaxed load and store) updates:

```text
release(o):
    if inert(o): return
    r = o.refs.load(relaxed)
    if r == IMMORTAL: return
    if live_threads.load(relaxed) == 0:
        if r == 1: queue o for destruction      # no fence: no other thread
        else: o.refs.store(r - 1, relaxed)
        return
    atomic path as today
```

Soundness: only the main thread runs while the count is zero, and it alone
changes the count from zero. Its plain updates before a `pthread_create`
happen before everything the new thread does; every update while workers are
live is atomic; `pthread_join` orders every worker update before the plain
updates that follow. A pool's idle workers keep the count above zero until
shutdown joins them. The destruction queue, `continue_after`, immortal counts
and actor heaps are unchanged.

Cost: one load of a read-mostly global and a predictable branch per count
operation, next to the existing inert and immortal tests. No marking, no
header change, no codegen change. Programs that start threads lose nothing
while workers are live.

### Stage 2: per-object thread-shared mark

Only if a parallel workload shows atomic count cost while workers run.

- **Encoding.** Bit 62 of `refs` marks a thread-shared object; `u64::MAX`
  stays immortal. One unsigned compare, `refs < SHARED_BIT`, selects the plain
  path for both retain and release; shared and immortal objects take the slow
  path, where immortal is tested first. Counts stay below `2^62 - 1`.
- **Marking.** At each "yes" boundary in the table above, the compiler emits
  a call to a marking walk for the crossing value's type. A type that cannot
  reach a heap `str`, a `Shared` box or an aliased helper result needs no
  walk: `list[i64]` or `Bytes` costs nothing, `list[str]` visits each heap
  string. The walk uses the destruction queue's worklist pattern, not
  recursion, and plain stores: the objects are thread-local until the boundary
  publishes them.
- **Store barrier.** Plenty mutates in place, so a marked container can later
  receive an unmarked value. Stage 2 marks values stored under a `Mutex`
  lock, and walks do not stop at already-marked objects. Stopping early would
  need a barrier on every store into a marked container, as in Ungar et al.
- **No unmarking.** A marked object stays marked, as in Lean and Koka: after a
  join another thread may still own a copy received through a channel.
- **Destruction.** The final release of a marked object keeps today's
  release/acquire handoff. Its children are marked, so the destroying thread
  updates them atomically. The queue link overwrites the mark only in a dead
  object, on its destroying thread.

Costs: the walk at each crossing, proportional to the reachable heap strings,
boxes and aliases (a scoped spawn over a large `&list[str]` walks all of it,
once per spawn); a compare per count operation, which replaces the `lock`
instruction on the common path; the `Mutex` store barrier.

## The sole-owner fast path

Issue #745 proposes that a release reading `refs == 1` destroy without an
atomic operation, and that a retain reading 1 store 2. The release half is
sound: the releasing owner holds the only count, and no borrow of that owner
can be live on another thread while it is released, so no other thread can
retain concurrently.

The retain half is not. Retains happen through borrows, not only from owners:

```text
parent owns s: str          # refs == 1
spawn(worker, &s)           # worker copies s: reads 1, stores 2
copy s on the parent        # concurrently reads 1, stores 2
```

Both threads store 2 for three owners, and the object is freed while one
copy is live. The same happens with two pool jobs copying an element of a
`Shared[list[str]]`. A plain retain needs a thread-locality guarantee, which
stage 1 provides for the single-threaded phase.

## Expected gain

Executed `lock`-prefixed count instructions, counted with callgrind
`--dump-instr=yes --compress-pos=no` at the addresses of `lock decq` in
`plenty_release` and `lock cmpxchg` in `plenty_retain`, on `main` at
`fe11d0a2` (release compiler, `--no-strip`):

| Program (input) | Release calls | Atomic releases | Final releases | Retain calls | Atomic retains |
| --- | ---: | ---: | ---: | ---: | ---: |
| k-nucleotide (fasta 50,000) | 4,320,706 | 1,815,225 | 222,025 | 2,569,772 | 1,315,181 |
| reverse-complement (fasta 50,000) | 1,536,704 | 1,536,704 | 3 | 1,536,701 | 1,536,701 |
| fasta (100,000) | 1,033,401 | 1,033,401 | 7 | 1,033,392 | 1,033,392 |
| regex-redux (fasta 50,000) | 412,613 | 281,838 | 246 | 232,686 | 231,533 |

The other benchmarks perform under 1,000 count operations each. The calls not
counted as atomic are inline strings and literals.

- Almost every atomic operation is a non-final pair at `refs >= 2`. In
  k-nucleotide the call sites in `count_frames` are the pair around
  `get_mut(&key)` (issue #744) and pairs around borrowed `Bytes` receivers;
  in reverse-complement all of them are the `Bytes.get`/`Bytes.set` receiver
  pairs of issue #741. Issue #745 handles the final releases: 12% of
  k-nucleotide's atomic releases, almost none elsewhere.
- Once those pairs are gone, stage 1 removes whatever atomic operations
  remain in these single-threaded programs. The size of that remainder is
  unknown until issues #744, #741 and #745 land; from the call sites it is
  expected to be small, so the additional gain on k-nucleotide and
  reverse-complement is likely a few percent at most.
- The upper bound, if pair removal covered none of them: in k-nucleotide at
  250,000, `plenty_release` and `plenty_retain` take 30.4% of cycles
  (issue #743); in reverse-complement about 60% (issue #741).
- Stage 2 gains nothing on these programs over stage 1. Its gain on parallel
  programs is unmeasured; BRC's 22.5% average for Swift client programs is
  an indication, not an estimate for Plenty.

## What to measure

1. Before and after each stage, the executed `lock` count-instruction totals
   above, the final-release count, and callgrind instruction totals for
   k-nucleotide and reverse-complement; then `bench.py run` timings.
2. A concurrency-heavy workload with workers live during the counting:
   a parallel k-nucleotide counting each frame on a pool job, as the Rust #1
   reference does; a channel pipeline of `str` messages; and readers of a
   `Shared[dict[str, i64]]`. Stage 1 should leave these unchanged; they decide
   whether stage 2 is worth building.
3. For stage 2, the marking cost: spawns in a loop over a large borrowed
   `list[str]`, and channel sends of `list[str]` messages.
4. Correctness: runtime tests in which several threads retain through a
   borrow, including the scenario above, and a stage-1 test that the counter
   returns to zero only after join and pool shutdown.

## Decisions

1. Stage 1 was accepted as the first step, then measured and rejected; see
   [measured result](#measured-result).
2. The live-thread count was to return to zero after the last join, decremented
   by the joining thread after `join` returns; detached or unjoined threads
   keep it raised.
3. Issue #745 dropped the retain fast path and landed the release half only.
4. The parallel k-nucleotide variant in `benchmarks/game` (local PR #940) is
   the threaded workload.
5. Any future thread that runs Plenty code, such as the proposed
   [actor blocking-work](actor-blocking-work.md) threads, must start through
   `plenty_thread_start`. This only matters if count modes return.

## Sources

- Lean 4 runtime: [`lean.h`](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h)
  (`lean_is_st`, `lean_is_mt`, `lean_dec_ref`) and
  [`object.cpp`](https://github.com/leanprover/lean4/blob/master/src/runtime/object.cpp)
  (`lean_mark_mt`, `alloc_task`, `lean_dec_ref_cold`); Ullrich and de Moura,
  *Counting Immutable Beans*, IFL 2019.
- Reinking, Xie, de Moura and Leijen,
  [*Perceus: Garbage Free Reference Counting with Reuse*](https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/),
  PLDI 2021, section 2.7.2 of the technical report.
- Choi, Shull and Torrellas, *Biased Reference Counting: Minimizing Atomic
  Operations in Garbage Collection*, PACT 2018,
  [doi:10.1145/3243176.3243195](https://doi.org/10.1145/3243176.3243195).
- [PEP 703](https://peps.python.org/pep-0703/): making the global interpreter
  lock optional in CPython.
- Ungar, Grove and Franke, *Dynamic Atomicity: Optimizing Swift Memory
  Management*, DLS 2017,
  [doi:10.1145/3133841.3133843](https://doi.org/10.1145/3133841.3133843).
- Swift: [SIL ownership](https://github.com/swiftlang/swift/blob/main/docs/SIL/Ownership.md),
  `lib/SILOptimizer/Transforms/CopyPropagation.cpp`, and the
  `test/SILOptimizer/assume_single_threaded.sil` test.
- Rust: [`std::rc::Rc`](https://doc.rust-lang.org/std/rc/struct.Rc.html) and
  [`std::sync::Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html).
- Nim: [memory management](https://nim-lang.org/docs/mm.html) and
  [`std/isolation`](https://nim-lang.org/docs/isolation.html).

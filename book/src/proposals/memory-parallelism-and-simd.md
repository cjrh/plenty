# Fallible memory, parallel execution, and SIMD

> Active tasks and priorities live in the [backlog](../backlog.md). Staging below
> records design dependencies, not scheduled work; some syntax and assumptions
> are historical. See the [implementation status](../design/04-implementation-status.md)
> and [reference](../design/index.md) for current behavior.

Status: **research and proposed direction, not implemented language behavior**.
Reviewed against Plenty after `b690afe`, with Cranelift `0.131.1`, on 2026-10-05.
All API names and syntax below are sketches. The tutorial continues to describe
the implemented language.

## Recommendation

Make recoverable allocation failure a language contract, with `Result` and `?`
as the normal way to handle it. This requires more than making collection growth
fallible: error values, metadata, cleanup, and runtime helpers must also work
without an allocation succeeding. Establish this foundation before advertising
OOM recovery, custom allocators, or embedded suitability.

Keep the public model simple: one `str`, ordinary collections using a configured
default allocator, and explicit constructors accepting an allocator when needed.
Start parallelism with owned tasks, bounded channels, and scoped borrowing; add
explicit parallel collection operations after those are sound. Start SIMD with
a small set of explicit fixed-width vectors. Neither automatic parallelization
nor a general auto-vectorizer should be necessary for the basic language.

## What the implementation actually does today

The runtime is Rust, but compiled Plenty is single-threaded and allocation
failure is not recoverable through a Plenty `Result`:

| Area | Current behavior and implication |
| --- | --- |
| Raw object allocation | `plenty-runtime/src/memory.rs::allocate` calls `alloc_zeroed`, then `handle_alloc_error` on null; layout overflow exits through `fail`. |
| Collection growth | `aggregates.rs::insert` uses `try_reserve` for entries, but turns failure into process termination. Hash-table rebuilding uses an infallible `vec!` allocation. |
| Type metadata | Descriptors are parsed into `String`, `Vec`, and `Rc<Type>` at runtime. Even constructing a small collection can allocate metadata. |
| Classes and enum values | Records have owned heap allocations. Standard `Result` and `Option` use enum machinery; constructing an error is not an allocation-free escape hatch. |
| Text and I/O | Dynamic text, input buffering, formatting, and collection rendering allocate. String literals are immortal compiler data. |
| Indexing and ranges | String indexing/iteration creates a new managed one-scalar string; constructing a range allocates metadata and a managed range object. |
| Structural equality | `aggregates.rs::equal` memoizes aggregate pairs in a Rust `HashSet`, avoiding exponential work on shared graphs whose floating-point contents prevent identity shortcuts. Equality and membership can therefore allocate internally. |
| Generators | Creating the frame allocates; suspended values own resources. Returning `Option` from iteration also uses enum machinery. |
| Destruction | An intrusive thread-local queue avoids allocating a separate worklist while traversing owned object graphs. User `__del__` code can still allocate. |
| Sharing | Object reference counts are non-atomic `u64`; metadata uses non-atomic `Rc`. Moving a collection does not prove its reachable immutable strings are unshared. |

Rust does have fallible APIs: `Vec::try_reserve` preserves existing contents on
failure. The familiar infallible APIs choose a different policy, and the standard
allocation-error handler normally aborts with `std`. Plenty can expose a
different policy while using Rust underneath, but must select or implement
fallible operations consistently. [Rust `Vec`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.try_reserve),
[allocation error handling](https://doc.rust-lang.org/std/alloc/fn.handle_alloc_error.html).

## Recoverable allocation contract

### Allocation-free error transport comes first

Introduce a small, non-owning `AllocError` value distinguishing exhaustion,
capacity overflow, and an unsupported allocation layout. Its data should be
fixed-size integers/tags, not an allocated message or backtrace. It can be
formatted later if resources permit; an emergency reporting path writes a
static message directly without growing a buffer.

Represent ordinary finite `Result[T, E]` and `Option[T]` values without a new heap
allocation for their wrapper: use an inline tag/payload layout, register tuples,
or a caller-provided result slot according to a private Plenty ABI. Moving an
already-created payload into `Ok` or `Err` must not allocate. This is a concrete
reason to generalize the current single-word managed-value representation.
Recursive values still need explicit indirection; neither inline errors nor
stack placement imply arbitrary recursive types have a finite layout.

Implementing `?` before this representation change is useful for normal errors.
It must not be described as OOM-safe while its generated error path still creates
heap-backed enum objects. Compiler-emitted immutable type metadata is the next
useful change: replacing runtime descriptor parsing removes hidden allocation
and a future cross-thread `Rc` hazard at the same time.

### Which operations return `Result`?

Recommend fallible public signatures for operations whose contract permits
allocation, even on calls that happen to fit existing capacity. Their type must
not change with optimization level, input size, or whether the compiler managed
to place a particular object on the stack.

For example, a future `items.append(value)?` propagates `AllocError`, and a future
`copy(value)?` can fail when duplicating owned data. Explicit `.expect(...)`-style
termination may be offered for applications choosing that policy; it is not a
silent default selected by release builds. A terminal failure is distinct from a
recoverable allocation error and does not promise user cleanup.

There is a syntax decision to make before changing existing programs: allocating
literals, comprehensions, string concatenation, heap-backed class construction,
and generator creation have no explicit fallible method today. A consistent
candidate is for these expressions to produce a `Result`, with explicit `?` at
the use site, including `([f(x) for x in source])?`. This makes allocation visible
but changes many familiar expressions and needs learner examples before adoption.
An alternative is a narrowly specified allocation effect with an explicit
handler boundary; it adds more language machinery and should not be quietly
introduced as implicit `?` insertion. No choice is finalized here.

The required behavior is clearer than the syntax: a program must be able to
handle every language-runtime allocation failure without an unadvertised abort.
Audit these paths together, not just `list.append`:

| Allocation site | Required recovery behavior |
| --- | --- |
| Lists, dicts, sets | Reserve entries and hash storage before committing a mutation; report checked-size overflow separately from exhaustion. |
| Comprehensions | Destroy the partial result and pending owned temporaries on failure. Already-completed I/O or other external side effects are not rolled back. |
| Strings | Concatenation, formatting, conversion, slicing that copies, and input buffering expose failure; literals and merely retaining existing immutable storage need no allocation. |
| String indexing and ranges | Remove their internal allocation through a suitable representation, or expose failure even though these operations currently look like simple value expressions. |
| Equality and membership | Replace allocating memoization with an allocation-free strategy, or make its failure explicit. A `bool` result must not conceal an abort when temporary comparison storage is exhausted. |
| Classes and enum payloads | Clean up only successfully initialized fields if construction fails. No call to a whole-object custom destructor on a partially initialized object. |
| `copy` | Destroy a partially built deep copy; retain the original unchanged. Resource objects with custom cleanup keep their existing copy restrictions. |
| Generators | Frame creation is fallible. Allocation errors in the body need an explicit yielded `Result` or a designed fallible-iteration contract, not an indistinguishable end-of-stream. |
| Metadata and dispatch | Prefer emitted static tables. Any remaining lazy caches or adapter allocation must return errors or be preallocated explicitly. |
| Printing and diagnostics | Stream through a nonallocating writer or propagate allocation/I/O errors. Formatting an error must not recursively allocate another error. |
| Scheduling and channels | Pool creation, task records, queue storage, thread stacks, and message construction have distinct fallible creation/submission contracts. |
| FFI | An external allocator/library can still abort; an imported declaration must not claim recoverable failure unless its implementation provides it. |

### Mutation, moved inputs, and cleanup on failure

Guarantee the strong property for allocator-aware collection mutation: if reserve,
rehash, or allocation fails, the existing collection's logical contents remain
unchanged. Capacity may have increased during preparatory work. A replace
operation must not destroy the old value until all required allocation succeeds.
Allocator failure must leave the original allocation valid on failed resize.

Recommend a simple initial ownership policy: an ordinary owning `append` consumes
its argument on the call, including failure, and destroys it if it was not stored.
The caller cannot conditionally reuse the moved binding after observing `Err`.
Document this prominently. A later recovering variant can return the uninserted
item together with `AllocError`, but that composite error must itself have an
allocation-free representation. Another explicit workflow is reserve first, then
move the value into already available capacity. Do not hide input cloning in the
error path.

Destruction itself must not allocate. The existing intrusive queue is a useful
starting point, not a complete proof: custom destructors, allocator callbacks,
formatting used by cleanup, and cleanup of partially initialized values all need
auditing. A destructor has no caller to receive `Result`; keep `__del__` and scope
exit hooks infallible and make fallible close/flush an explicit operation before
drop. OOM-safe cleanup must handle any fallible work locally and continue releasing
owned resources. A later compiler-checked `noalloc` effect could verify critical
cleanup and request-rejection paths; initially keep this a documented runtime
contract with tests, not an unimplemented guarantee. See the companion
[context-manager proposal](protocols-generics-and-context-managers.md).

The runtime needs a failure-injection allocator: fail allocation number N for
every N in a test, then verify unchanged source values, balanced resources,
partially initialized cleanup, and no secondary allocation on the failure path.
Include failures during hash growth, nested copies, generators, output, and user
cleanup. Success-only accounting and sanitizer tests cannot establish these
failure guarantees.

### What recovery can and cannot guarantee

An allocator reports only failures it can observe. On an overcommitting OS,
successful reservation can be followed by process termination when pages are
touched; a `Result` cannot intercept the kernel killing the process. Bounded
allocators and request budgets make admission control more useful; reserved and
committed-memory policy remains platform-specific. Linux explicitly documents
multiple overcommit policies. [Linux overcommit accounting](https://docs.kernel.org/mm/overcommit-accounting.html).

Compiler OOM, process startup, stack overflow, and third-party C libraries are
separate limits. Do not advertise survival of arbitrary system-wide exhaustion.
For services, the practical guarantee is narrower and useful: reject work when a
controlled allocator cannot satisfy a request, clean up, and continue using
previously reserved service resources. An application needs a preallocated path
for that rejection response too.

## Custom allocators without multiplying everyday types

The user's proposed separation is viable. Prefer one ordinary constructor with
an optional allocator argument over unrelated `list_custom`/`dict_custom` names.
Exact syntax depends on generics, but conceptually `list[T](allocator=pool)` uses
the same operations as `list[T]()`; literals and comprehensions use the configured
default. An explicit collection builder can later direct a comprehension into a
particular allocator without introducing ambient thread-local allocation scope.

Use a small runtime allocator handle containing a context pointer and operations
for allocate, grow/resize, and deallocate. The contract includes alignment,
checked sizes, thread eligibility, failure behavior, and ownership of the handle.
This is compatible with statically known adapters for user classes; it does not
require every ordinary collection type to expose an allocator generic parameter.
The tradeoff is a stored handle and indirect allocation calls. Specialized
allocator-generic collections can remain a later optimization.

Rust's per-object `Allocator` contract is useful prior art, including matching
deallocation and allocator lifetime, but the inspected stable documentation still
marks it an unstable API. Plenty can implement its own stable runtime interface
without exposing Rust's allocator ABI. Rust's `GlobalAlloc` also permits an
implementation that aborts instead of returning null: a Plenty allocator
advertised as recoverable must impose the stronger return-failure rule.
[Rust `Allocator`](https://doc.rust-lang.org/std/alloc/trait.Allocator.html),
[Rust `GlobalAlloc`](https://doc.rust-lang.org/std/alloc/trait.GlobalAlloc.html).

The difficult part is lifetime and provenance, not constructor spelling:

- Each object and internal buffer remembers the allocator that created it, and
  frees through that allocator even if the default later changes. Growth stays
  with that allocator. Start with default selection fixed at startup to avoid
  unnecessary ambient state and initialization races.
- The allocator's context must outlive every live allocation and destructor using
  it. Start with process-lifetime or explicitly retained allocator handles. Handle
  creation is fallible and its own allocation must not form an ownership cycle.
- A scoped arena is a borrow-checking feature: forbid reset/destruction while any
  arena-backed value or reference can survive. Deferring resettable arenas is
  better than using a runtime pointer that silently dangles after a scope ends.
- An arena does not make resource cleanup optional. Destructors of files and
  other resources still run; bulk byte reclamation cannot replace them.
- Inserting an existing string or nested list into a pool-backed list does not
  migrate its storage. The outer buffer uses the pool; each child retains its own
  allocator. A future explicit `copy_into(pool)` must define which reachable
  storage it duplicates and may fail partway through.
- `copy` should default to the source allocator; changing allocators is explicit.
  No pointer allocated in a DLL may be passed to another DLL's unrelated free
  function. FFI ownership contracts need the matching release operation and its
  library lifetime; Plenty's allocator handle remains a private representation.
- Thread-confined allocators make their values thread-confined. Merely making a
  reference count atomic cannot make an allocator or its destructor thread-safe.

Supporting arbitrary allocator callbacks is itself a trusted low-level extension;
the compiler cannot prove that foreign callbacks honor alignment and lifetime.
Built-in budget/pool allocators should be the first supported implementations.

### Fixed capacity and inline storage

The ideas in `heapless` and `smallvec` solve different problems. `heapless`
containers store fixed-capacity backing storage inline and report capacity
exhaustion; `SmallVec` stores a small prefix inline and spills to the heap when
needed. Inline means inside the containing value, not necessarily on the stack.
[heapless](https://docs.rs/heapless/latest/heapless/),
[SmallVec](https://docs.rs/smallvec/latest/smallvec/struct.SmallVec.html).

Possible later Plenty types are `Array[T, N]`, `BoundedList[T, N]`, and
`SmallList[T, N]`, with constant generic capacities. These names are illustrative.
A bounded container returns a capacity error without allocating; a small list
still needs fallible spill/growth. Element construction can allocate even when
the container does not. A borrowed slice/view should let APIs accept their
contents without copying into the ordinary heap-backed list. Defer this family
until inline aggregates and basic constant parameters exist; it should not
complicate the first custom-allocator interface.

## Threads, channels, and parallel collections

### Runtime prerequisites

The [native concurrency audit](native-concurrency.md) records the current runtime
and boundary contract. Type metadata is already static and immutable. Object
reference counts remain non-atomic, including immutable strings: a list move
may leave a string alias in the originating thread. Atomic ownership alone also
does not make a mutable payload safe to share.

Retain static immutable type metadata and use atomic reference counts for storage
that can be shared between threads, including the single public `str` type.
Benchmark the cost before choosing whether all headers should use the same atomic
implementation or whether internal unshared objects can keep a cheaper path.
Either choice preserves one user-visible `str` type. Revisit the destructor queue
algorithm when changing the count field: zero-to-destruction handoff, memory
ordering, and recycling the dead count as a link need a coherent atomic design.

Define compiler-known structural properties for **transferable ownership** and
**shareable immutable borrows**. These are safety facts, not protocols users can
satisfy merely by writing a method with the right name. Ordinary methods remain
explicitly resolved; importing a module must not enable extra thread eligibility.
Check all captured values recursively, including allocator handles, generator
frames, destructor requirements, and FFI handles with thread affinity.

### Initial user model

1. Owned tasks move captures and results. Thread creation returns `Result`, and
   join is explicit; no silent detached thread holding resources beyond `main`.
2. Scoped tasks may borrow until the scope joins every task on every exit path,
   including `?`. Shared borrows need thread-shareable values; mutable borrows
   require disjoint storage proven by the checker. Scoped threads are established
   prior art in [Rust's `thread::scope`](https://doc.rust-lang.org/std/thread/fn.scope.html).
3. Bounded channels own preallocated queue storage. Sending transfers ownership;
   failed or disconnected send returns the undelivered value. Creation and
   payload construction are fallible; preallocating the queue does not eliminate
   allocations inside payloads. Specify closure, blocking, and shutdown behavior
   before adding unbounded channels.
4. A configurable thread pool supports explicit parallel map/collect/reduce.
   Keep the executor API replaceable: the language need not promise Rayon as a
   dependency, and any chosen executor must meet the allocation contract.

Rayon demonstrates explicit parallel iterators and configurable pools, including
`num_threads`; its parallel fallible traversal may observe multiple errors and
does not specify which one is returned. These are useful comparisons, not a
reason to inherit unspecified language behavior accidentally.
[Rayon pool configuration](https://docs.rs/rayon/latest/rayon/struct.ThreadPoolBuilder.html#method.num_threads),
[Rayon fallible traversal](https://docs.rs/rayon/latest/rayon/iter/trait.ParallelIterator.html#method.try_for_each).

For an initial Plenty parallel collection operation, preserve input order in the
successful result. If multiple items fail, returning the failure at the lowest
input index is easier to reproduce; it requires completing earlier in-flight
work before reporting it. Cleanup drops all partial outputs and joins started
tasks. It cannot undo I/O already performed by workers. A simpler first-completed
error mode is an alternative, but must be explicitly documented if selected.
Dictionary/set construction must still resolve duplicates using source order.
The order of destructor side effects across threads is not the serial iteration
order and must not be promised accidentally.

Cancellation is cooperative and happens at defined boundaries, not by killing
threads during mutation or drop. Restrict `break`/`continue` in higher-level
parallel loop forms until their semantics are specified. Floating-point
reductions need an explicit order policy: reassociation changes results; a
parallel sum cannot silently claim ordinary left-to-right arithmetic semantics.

Automatic parallelization is deferred. Borrow independence alone does not prove
equivalence: allocation failures, I/O, externally visible destructor order,
floating-point reductions, and first-error ordering are observable. Start with
explicit opt-in operations, measure workloads, then consider an effect analysis
for a small deterministic subset. A runtime thread-count setting is reasonable
for these operations, but should not silently change ordinary loop semantics.

## SIMD that fits Cranelift

Cranelift represents lane-typed vectors and provides vector arithmetic, but an
IR type's existence does not promise every instruction or width works on every
backend. The upstream IR guide explicitly discusses target-dependent support.
[Cranelift IR](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md),
[instruction builder](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/ir/trait.InstBuilder.html).

The installed `cranelift-codegen 0.131.1` source was checked directly:
`src/ir/types.rs` tests cover `I32X4` and `F64X2`; `src/isa/x64/lower.isle` includes
`fadd` rules for `F32X4`/`F64X2`; `src/isa/aarch64/lower.isle` lowers vector `fadd`.
This is evidence for a small 128-bit starting set, not a blanket claim about
256/512-bit vectors, scalable vectors, arbitrary lane counts, or all operations.
The docs.rs pages for that exact crate version were unavailable to the research
tool, so the source inspection, rather than latest-version docs, establishes the
version-specific finding.

Propose explicitly imported vector types/functions first: 128-bit shapes such as
four `f32` lanes or two `f64` lanes, with splat, checked load/store, arithmetic,
comparisons, masks, and lane extraction. Names such as `simd.f32x4` are sketches.
Represent vectors inline in SSA/registers or stack slots; do not heap-box each
vector as today's general managed objects are boxed. Efficient vectorized kernels
also need contiguous typed buffers/views: the current generic collection entry
storage is not a guarantee of a contiguous C array of lane values.

Specify exact integer overflow behavior, mask representation, lane order,
alignment, bounds, and floating-point behavior. Scalar fallback must preserve
those semantics, not merely produce approximately similar answers. Unaligned
loads are a useful safe default where supported; never read past a slice end to
fill a vector, even if discarded lanes appear unused. Keep scalar tails explicit
at first. SIMD reduction order and NaNs must follow the documented operation;
relaxed reassociation and fused arithmetic require opt-in semantics.

Compile a documented baseline target with scalar fallback for unsupported
operations. Offer explicit native-target compilation separately, recording the
required CPU features; shipping native-target binaries to an older CPU can fail.
Runtime feature dispatch and multiple compiled variants are later library/compiler
features with code-size and compile-time costs. SIMD vectors should not cross the
initial public C ABI by value: use pointers and lengths until each target's ABI
and alignment rules are deliberately supported.

Before adopting a vector operation, test native and scalar implementations on
boundaries, NaNs/infinities, overflow, short/unaligned buffers, and each supported
target. Inspect generated code and measure throughput and compilation time.
Explicit vector lowering is compatible with Plenty's fast-compilation goal;
global loop dependence analysis and speculative vectorization are not required.

## Suggested implementation sequence

1. Add ordinary `?` and clarify cleanup on propagated errors. Explicitly document
   that current heap-backed `Result` is not yet an OOM-recovery mechanism.
2. Design allocation-free finite sum/aggregate layouts and emitted static type
   metadata; measure compile time and ABI complexity before wider runtime changes.
3. Introduce a fallible allocation ABI and failure-injection tests, then migrate
   every runtime allocation path. Settle allocating-expression syntax with small
   tutorial examples before changing all public APIs.
4. Add a configured default allocator and explicit per-container allocator handles,
   starting with a bounded budget allocator. Keep scoped resettable arenas deferred
   until lifetimes can be enforced.
5. Add contiguous buffers/views and bounded inline containers as generic/value
   support permits. These also establish a practical foundation for SIMD.
6. Audit thread safety and add transfer/share checking, atomic shared ownership,
   owned/scoped tasks, and bounded channels. Then explicit pool-based parallel
   collection operations with documented ordering and failure behavior.
7. Add the narrow tested SIMD surface. Automatic parallelization, auto-vectorization,
   allocator specialization, and runtime CPU multiversioning remain optional later
   work rather than prerequisites for useful programs.

Important open decisions are allocating-expression syntax, a coherent private
sum-value lowering contract, allocator handle lifetime representation, whether failure recovers
owned insertion arguments, task error ordering, and which SIMD operations have a
portable scalar contract. None should be hidden behind a promise that the current
runtime already supplies these capabilities.

The private lowering contract is not a frozen ABI or a public C representation;
it must leave room for future FFI, vector values, target-specific calling
conventions, and further representation improvements.

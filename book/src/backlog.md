# Backlog

This is the single maintained list of planned language, compiler, runtime, and
tooling work. It replaces the separate roadmap and next-work lists. Entries are
candidates, not promises about syntax or delivery dates. Allocation-free ranges
and concrete generator frames are implemented independently.

Use the [implementation status](design/04-implementation-status.md) to see what
works today, the [reference](design/index.md) for its exact contract, and the
[proposals](proposals/index.md) for design rationale and alternatives. A proposal's
stages describe technical dependencies, not a second priority list.

## Next batch

The initial toolchain and C boundary batch (B01–B07) is complete, including typed
runtime loading with resident code, recoverable LoadError values, and ownership
wrappers checked against their originating library instance.

B08's initial callable and closure feature set is complete: allocation-free thin
functions and concrete environments, checked owned/shared/exclusive captures,
signature inference, factories, inline sums, reusable and consuming callable
constraints, and explicit one-shot closures. Native tests cover moves, suspended
generator relocation, and normal/early/failure cleanup with allocation disabled.

B09's initial generic data and protocol batch is complete: concrete classes and
enums, inferred construction, generic methods, parameterized structural protocols,
and signature-driven inference. Native failure/lifecycle tests and bounded
specialization tests cover the implemented subset.

B30 is complete: nested list/dictionary/field writes save RHS and index values
before resolving addresses. Native checks cover disabled allocation, resizing
index expressions, conflicting loans, invalid paths, propagation, and owner drops.

B31 is complete: the [callable surface review](proposals/callable-simplicity.md)
records a concrete reason for retained representation and invocation distinctions.
The tutorial now offers a short callback path and uses the common `Callable`
constraint for generic consumers; factory and consuming annotations are advanced.

B22's contract/runtime audit and atomic ownership foundation are complete.
Metadata is static, and counts synchronize last-release cleanup, including inline
frames. Concurrent stress tests cover the handoff. Initial compiler eligibility
checks and scoped native threads are also complete: borrowed/owned jobs,
recoverable failed starts, transferred results, and automatic joining on every
normal exit. Bounded channels and endpoint guards now compose with those scopes. See the
[reference](design/32-scoped-native-threads.md) and
[runnable lesson](tutorial/79-run-scoped-threads.md).

B10 is complete for the initial source subset: classes, enums, aliases, and finite
generic instances support recursive ownership through `Box` and collections.
Native tests cover construction, consuming matches, borrowing/moves, allocation
failure, allocation-free replacement, and deep drop. Imports preserve visibility; static and shared libraries preserve
opaque C handles and generated ownership wrappers. The
[runnable lesson](tutorial/77-build-recursive-data.md) teaches chains and trees.
See the [implemented contract](design/30-recursive-data.md) for precise limits.

B11's borrowed enum matching slice is complete. Shared matches preserve owners;
mutable matches support inline sums and affine enum payloads. Payload loans protect
variants, support single-origin returns, and allocate nothing, including nested
`Option`/`Result` projections. Copyable heap enum storage remains immutable.
See the [runnable lesson](tutorial/78-match-borrowed-values.md) and
[implemented contract](design/31-borrowed-enum-matching.md).

**B22's committed concurrency batch is complete**, building on the
[thread-pool executor](design/36-thread-pool-executors.md). Fixed workers,
bounded owned submission, recoverable jobs, affine futures, pending cancellation,
ordered list/range mapping, and context-managed drain/join are implemented.
Explicit [fallible mapping and fixed-tree reduction](design/37-parallel-operations.md)
are implemented too. The final three slices are implemented:

1. [Cooperative cancellation tokens](design/38-cooperative-cancellation.md) for running tasks.
2. [Timed channel operations and non-consuming future waits](design/39-timed-waits-and-selection.md).
3. Allocation-free receive selection between two typed channels.

Zero-capacity rendezvous, broader iterator/closure mapping, streaming results,
richer certified effects/types, and automatic parallelization (B24) are explicitly
deferred beyond this batch. Arbitrary-size and send-side selection are also
deferred; the initial receive pair keeps heterogeneous messages statically typed.
**Next: B34, cleanup and taking stock**, before starting unrelated feature work.
The concurrency extensions above, including B24 research, are explicitly deferred;
they do not postpone this review.
Broader stored-reference work remains under B11.
Automatic deep recursive
copy, equality, and formatting remain explicitly gated and are tracked by B33.

Named function values already support basic callback registries: a
`list[Callable[[i64], i64]]` can hold named functions, including inside a class,
and `callbacks[index](value)` calls the selected function. These are indirect
calls with a fixed signature. Concrete owned environments now also fit in classes,
tuples, lists, dictionaries, and user enums; see
[stored closures](design/33-stored-closures.md). Different arbitrary environments
still cannot be erased behind one callable signature. See also
[function values](design/25-function-values.md).
Explicit state beside a named callback also supports stateful registries today;
the [runnable lesson](tutorial/76-store-stateful-callbacks.md) and native tests
cover generic state, allocation-free dispatch, and exactly-once resource cleanup.

The completed B32 batch comprised these ten reviewable tasks:

1. **Complete:** choose the owned concrete storage contract; compare erasure costs.
2. **Complete:** generalize allocation-free collection extraction to type-sized inline payloads.
3. **Complete:** store reusable owned environments in generic class fields.
4. **Complete:** support owned environments in tuple slots and checked disjoint borrows.
5. **Complete:** support homogeneous closure lists, including growth and extraction.
6. **Complete:** support closure dictionary values and replacement/removal cleanup.
7. **Complete:** support concrete closure payloads in user enums.
8. **Complete:** extend these storage paths to owned consuming closures (excluding generator captures).
9. **Complete:** check callable constraints on generic data declarations, including inferred signatures and public visibility (B29).
10. **Complete:** integrate stored environments with scoped-thread eligibility, borrowing, and allocation-free callback result transfer.

The [storage decision](proposals/stored-closures.md) preserves concrete identities
and ordinary fallible container construction. Heterogeneous erased callbacks and
escaping borrowed environments remain separate B32/B11 work. Generator-containing
environments remain excluded from heap storage pending layout/lifecycle support.

### Scope of follow-on work

**B11 — broader reference relationships.** Borrowed enum matching and payload
loans are implemented. Remaining slices include stored references, relationships
among multiple reference parameters, and more precise collection loans. Preserve
owner/variant stability and reject references into temporary storage as this
subset expands; evaluate additional precision against compile-time cost.

**B22 — useful native concurrency.** Scoped native threads, structural eligibility,
reachable worker/destructor effect checks, and atomic ownership are implemented.
Jobs may borrow owners or consume a concrete owned closure; failed consuming
starts return the unstarted job without allocation. Results transfer at join.
Positive-capacity MPMC channels are implemented with recoverable construction,
blocking/nowait transfer, failed-send ownership, and endpoint guards. The initial
Python-style executor now supports bounded submission, result retrieval, eager
ordered list/range mapping, pending cancellation, and context-managed shutdown.
Fallible ordered mapping is implemented with deterministic earliest-input errors
and partial-result cleanup; see [parallel operations](design/37-parallel-operations.md).
Fixed adjacent-pair tree reduction is also implemented, including allocation-free
empty/singleton paths and fallible scratch storage. Cooperative cancellation
tokens, timed waits, and two-channel receive selection complete the committed batch.
Broader iterator/closure mapping, streaming results, richer certified effects/types,
zero-capacity rendezvous, and broader selection are deferred beyond B34.
Thread/task creation and queue allocation must report failures; specify bounded
queues/backpressure, failed-send ownership, joining on every exit, cooperative
cancellation, and partial-result cleanup. Executor result handles do not require
async/await. The initial explicit parallel operations are complete;
automatic parallelization remains B24. Prefer library APIs over new syntax, with
compiler support for ownership checks and runtime support for native threading.
Scoped generic tasks need not wait for heterogeneous closure storage in B32.

**B32 — broader callback storage.** Owned concrete environments now work in heap
owners; [the storage decision](proposals/stored-closures.md) records why this adds
no erased callable type. Remaining heterogeneous environments behind one callable
signature need an explicit representation and dispatch contract. Evaluate
fallible owned boxing and bounded inline storage against actual registry/event
handler needs before adding public types. Specify reusable/consuming invocation,
move/drop behavior, allocation failure, and capture lifetimes. Begin with owned
environments; escaping borrowed captures depend on B11. Keep Plenty callback
storage distinct from C ABI callbacks and foreign-library lifetimes in B28.

**B34 — post-concurrency cleanup and review.** Review the implementation and book
together, take stock of supported behavior and limits, and simplify unnecessary
complexity before the next feature batch. Search `issues.db` before recording
defects or improvement opportunities; address open issues in severity order and
link scheduled fixes here by issue number. Review documentation structure,
navigation, duplication, and terminology; rewrite for concise, precise explanations
and a sensible learning sequence. Keep runnable lessons, reference contracts, and
implementation aligned, and validate the book and regression suite after cleanup.
Record deliberate deferrals here rather than creating another work queue.

Current cleanup slice: **issue #15**, reject implicitly discarded Results and
direct Result arguments to `print`, with propagation advice and updated examples.
Completed: **issue #16**, report an `Err` returned by `main` on standard
error, and give `IoError` distinct variants for failures without an OS code.
`Failure` still keeps no details; storing a cause in it belongs to B14.

## Work items

The order above selects the next slices. This table is the catalog of open items,
including later candidates; its row order does not set priority. Detailed syntax
is still open.

| ID | Work | Scope / dependency |
| --- | --- | --- |
| B11 | Broader borrowing | Borrowed enum matching and payload loans complete. A `mut` reference binding is reassigned only through itself (issue #1). Remaining: stored references, relationships among multiple reference parameters, more precise collection loans, retargeting a reference binding to another borrow of the same owner, and field-precise loans through `mut` reference bindings. B30 completed ordinary nested writes; disjoint-index precision remains here. Evaluate precision against compile-time cost; the current single-parameter returned-borrow rule is documented in the [reference](design/18-returned-references.md). |
| B12 | Practical I/O and iteration | Fallible file iteration, explicit binary buffers and read/write, buffering, and reusable iterator/view protocols. Follow familiar Python conventions where they fit ownership and Result-based errors. |
| B13 | Public allocator control | Global/default and per-container selection, allocator state/lifetimes, and buffer provenance. Internal object allocator identity already exists. Consider bounded/inline-capacity storage separately. [Memory design](proposals/memory-parallelism-and-simd.md). |
| B14 | Typed error composition | Preserve details across multiple error types through explicit unions/conversions. `Failure` already supplies deliberate erasure; it does not replace recoverable typed errors. |
| B15 | Enum-pattern shorthand | Infer a user-defined enum's qualifier from the matched value, so arms need not repeat it. Define ambiguity and name-resolution rules. Catch-all `case _:` is already implemented. |
| B16 | Additional inference and value conveniences | Consider expected-result inference, partial explicit type arguments, and contextual literals across generic arguments. Now that tuples are inline, decide whether copyable tuples may be borrowed mutably, and whether scalar-only classes copy rather than move (issue #17). Standalone unit bindings/parameters/storage, nested unpacking, and slice syntax/steps also need concrete use cases and contracts. These remain candidates, not automatic extensions of today's inference rules. |
| B17 | Modules and packaging | Package/dependency configuration, separately cached module objects, re-exports, and import-cycle policy. Preserve absolute imports, visibility, and deterministic name resolution. [Module design](proposals/entrypoints-modules-and-tutorials.md). |
| B18 | Windows and additional targets | Target-specific runtime support, linker-driver conventions, object/executable naming, and native CI. Depends on target validation and packaging work; changing the linker alone is insufficient. |
| B19 | Measured codegen and storage improvements | Benchmark dense-match dispatch before adding jump tables. Inline record fields use 16-byte slots and copies go through a runtime call (issue #17); measure natural field packing and inline copies. Investigate packed numeric buffers, code size, and compilation/link latency. Do not carry over assumptions from the legacy stack frontend. |
| B20 | Optional GCC backend | Evaluate the proposed backend with a small feasibility experiment before committing to another backend or common IR. Cranelift remains the current backend. [GCC proposal](proposals/codegen-gcc.md). |
| B21 | Documentation authoring | Evaluate independently runnable literate lesson sources that generate mdBook pages if they improve authoring. Current Markdown lessons are already executable and tested; do not create another manually maintained copy. [Documentation design](proposals/entrypoints-modules-and-tutorials.md). |
| B22 | Threads, channels, and parallel operations | Committed batch complete: eligibility/effect checks, scoped native threads, consuming jobs, bounded channels, executors, explicit fallible mapping/tree reduction, cancellation tokens, timed waits, and receive-pair selection. Broader mapping, richer effects, rendezvous, and broader selection are deferred beyond B34. Preserve fallible submission and deterministic shutdown. [Threads](design/32-scoped-native-threads.md); [channels](design/35-bounded-channels.md); [executor](design/36-thread-pool-executors.md); [parallel operations](design/37-parallel-operations.md); [cancellation](design/38-cooperative-cancellation.md); [timed waits and selection](design/39-timed-waits-and-selection.md). |
| B23 | SIMD | Define vector values, contiguous numeric storage, supported operations, and scalar fallbacks. Ordinary generic lists are not automatically packed native arrays. [SIMD design](proposals/memory-parallelism-and-simd.md). |
| B24 | Automatic parallelization | Explicitly deferred beyond B34. Require evidence that effects, cleanup, allocation failures, and result ordering remain correct. A runtime thread-count setting alone does not establish those guarantees. |
| B27 | Expanded C representations | Export text/byte buffers and borrowed views, broader typed errors and recoverable returned ownership, and explicit class-method adapters. Keep allocator provenance, output initialization, and error-path transfer visible. Initial class factories support AllocError, which reports allocation of the boxed handle. See [owned exports](design/24-c-interfaces/03-owned-exports.md). |
| B28 | Broader library loading and lifecycle | Generate runtime loaders directly from extracted contracts without original export source; improve error detail and binding tooling for third-party C libraries. Unloading, retained/foreign-thread callbacks, C record layout, and header-assisted bindings need separate contracts. Current loaders keep mappings resident and require their creating thread. [Runtime loading](design/24-c-interfaces/06-runtime-loading.md). |
| B29 | Further generic constraints | Callable bounds on data are implemented. Consider protocol bounds on data declarations, generic methods satisfying protocol requirements, combined bounds, and protocol composition when concrete library APIs need them. Generic aliases and enum constructor inference also remain candidates. Preserve explicit lookup and bounded specialization. See [generic data](design/28-generic-data-types.md) and [parameterized protocols](design/29-parameterized-protocols.md). |
| B32 | Broader callback storage | Owned concrete reusable/consuming environments and their worker eligibility are implemented. Remaining: heterogeneous erased environments and generator-containing environments in heap owners. Escaping borrowed storage depends on B11. See scope above. |
| B33 | Deep operations on recursive values | Recursive source storage is implemented under B10. Replace native recursive copy/format traversals with iterative fallible work storage before enabling them; select an explicit bounded-memory contract for structural equality and membership without adding hidden allocation failure. Preserve failure cleanup, shared-string ownership, and float/NaN semantics. [Design rationale](proposals/recursive-data.md). |
| B34 | Post-concurrency cleanup and review | Required after the thread/parallel/concurrency batch and before unrelated feature work. Audit code and docs, track findings in `issues.db`, fix in severity order, simplify implementation, and improve book structure and concise, precise writing. See scope above. |

## Reconciled completed work

These old backlog entries are closed, not additional tasks. The linked reference
pages own the current details; this table records why they disappeared from the
active list.

| Previous item | Resolution |
| --- | --- |
| B35: Inline values | Classes, tuples, and enums store their fields inline, so construction never allocates and needs no `?` (issue #17). Recursive types use `Box[T]`, which converts to its content wherever the content's type is required; recursion without a heap boundary is rejected. C export handles box their instance. See [classes](design/12-classes-fixed-layout-records.md) and [recursive data](design/30-recursive-data.md). |
| B10: Recursive source types | Self/mutual recursive classes and enums, aliases, and finite generic instances reach themselves through boxes and collections (originally heap records; see B35). Finite compiler graphs avoid type-table leaks; native source tests drop 100,000-node class and enum chains without allocation on a 256 KiB stack. Runnable lessons, module visibility, and C/Plenty library consumers cover the initial subset. Borrowed matching is B11; automatic deep operations are B33. |
| B09: Initial generic data types and richer protocols | Concrete generic classes/enums, IntType data bounds, function/constructor inference, inferred/explicit generic methods, parameterized structural contracts, and protocol-driven inference are implemented. Native allocation-failure/drop checks and specialization reuse/limits cover the initial subset. Extensions are B29; recursive storage was completed under B10. |
| B08: Function values and multiline closures | Thin functions, reusable/consuming concrete environments, explicit captures, factories, signature inference, `Callable`/`OnceCallable` constraints, inline sums, captured generator frames, and checked lifecycle cleanup are implemented without implicit allocation. B31 reviewed the surface and simplified its teaching path; captured-environment storage remains B32 and stored/escaping borrows remain B11. See [closures](design/26-closures.md) and [consuming closures](design/27-consuming-closures.md). |
| B30: Nested assignment | Existing list/dictionary/field slots update without enclosing copies or allocations. RHS and saved indices precede address resolution; tests cover resizing, aliases, failure cleanup, and owner replacement. |
| B31: Callable simplicity audit | Retained concepts have explicit use-case rationale; the tutorial teaches inferred local closures and one reusable generic callback interface before advanced factory/consuming annotations. Ordinary generic records demonstrate stateful registries without another callable type. |
| B07: Initial runtime library loading | CLI/API generation emits self-contained typed loaders; exact discovery metadata and all symbols are checked before returning a table. Scalar/borrow/Result adapters and owned factories/transfers preserve signatures, destruction, and instance provenance. Allocation-free LoadError, injected failures, private-address checks, and the executable library tutorial cover the initial resident implementation. [Runtime reference](design/24-c-interfaces/06-runtime-loading.md). |
| B06: Initial C library exports | Static/shared packaging, scalar and Result adapters, owned factories, matching destruction, shared/mutable and consuming handles, precise C contracts, embedded/extractable interfaces, SHA-256 link guards, explicit binary verification, and staged publication are implemented. Native callers and injected failures validate cleanup. [Library reference](design/24-c-interfaces/02-library-exports.md). |
| B05: Ownership-aware C adapters | Scalar/pointer borrows and explicit UTF-8/C-string adapters integrate with normal borrowing. Private opaque pointers in classes provide owned handles and matching destruction; C fixtures verify partial acquisition, allocation errors, and conditional/unconditional transfer. Mutable/returned byte buffers remain outside this subset. See [adapters](design/24-c-interfaces/01-ownership-and-text-adapters.md). |
| B04: Initial C imports | Trusted `.plentyi` declarations support sized C scalars and nominal opaque pointers. Native C fixtures cover static/shared linking and actual ABI calls; visibility and Plenty layouts remain separate. See [C interfaces](design/24-c-interfaces.md). |
| B03: Target compatibility | Native compilation validates the supported x86_64 Linux GNU target, packaged runtime triple, and ISA pointer width. Explicit target selection rejects incompatible layouts before source loading/emission; runtime extraction includes target metadata. See [runtime packaging](design/09-rust-runtime-packaging.md). |
| B02: Object-file output | CLI/API application object emission and matching runtime extraction work without a linker on PATH. External-driver tests link and run single-file and imported programs. See [execution commands](design/07-execution-commands.md). |
| B01: Linker selection | CLI `--linker` / `--link-arg` and Rust `CompileOptions` select a cc-compatible driver without shell parsing. Native and mock-driver tests cover argument fidelity, failure diagnostics, and temporary cleanup. See [execution commands](design/07-execution-commands.md). |
| B25: Allocation-free generator frames | Generator calls return concrete inline frames directly. Consumers specialize by producer identity; factories and standard sums preserve that identity. Native tests disable heap allocation across creation, moves, calls, nested frames, resume, and cleanup. Recursive inline layouts are rejected. See [native generators](design/16-native-generators.md). |
| B26: Allocation-free ranges | Range construction now returns a copyable inline value directly. Calls, returns, standard sums, indexing, membership, and repeated traversal need no range allocation. Stored ranges live in their containing owner's storage. See [collections and iteration](design/05-current-language-contract/08-collections-and-iteration.md). |
| String/heap reclamation | Owned values and strings are reclaimed by the Rust runtime, with deterministic cleanup. See [ownership](design/13-ownership-and-reclamation.md) and [destruction](design/14-deterministic-destruction.md). |
| Length-aware strings | Strings already store explicit lengths and support embedded NULs. The original `strlen`-based premise is obsolete; adopting a two-word source value is not required to resolve it. |
| Precompiled runtime archive | The runtime archive is built with the compiler and embedded for linking. It is not rebuilt from C source for each program. See [runtime](runtime.md). |
| Foundation batch | Main/imports/visibility, fallible allocation APIs, borrowing, tuples/items, typed ranges, generic functions, and structural protocols have implemented paths. See the [historical foundation record](proposals/next-work-queue.md) and [current status](design/04-implementation-status.md). |
| Allocation syntax and error convenience | Allocating operations return Result by default; `?`, explicit success values, and `Failure` replace the old `try`/`try_` alternatives and repetitive unwraps. |
| Generic argument inference | Calls infer concrete type parameters from arguments; explicit arguments remain available. See [generic functions](design/02-generic-functions-and-argument-inference.md). |
| Catch-all matching | `case _:` already handles remaining enum variants. See the [enum lesson](tutorial/18-describe-alternatives-with-enums.md). |
| mdBook migration | Reference and tutorial pages live under `book/src/`, are listed in `SUMMARY.md`, and tutorial examples run in tests. Literate-source generation is the separate B21 candidate. |

## Keeping this list useful

- Add, prioritize, split, or mark active work here. Keep IDs stable; link design
  detail instead of copying a task list into a proposal, tutorial, or reference.
- When selecting a candidate, settle its scope and acceptance checks. The table
  order does not approve every proposed API or require all later candidates.
- Before adding syntax or builtins, show the user problem, why ordinary library
  code cannot solve it, and the ownership, allocation, compilation, and teaching
  costs. Ease of implementation alone is not a reason to enlarge the language.
- On completion, remove it from the active queue and update implementation
  status, the relevant reference pages, and runnable lessons in the same change.
  Retain a short completion note here only when it explains a retired entry.
- Use the [development workflow](design/23-next-milestones.md) for shared testing,
  documentation, and performance expectations. Those are ongoing practices,
  not separate feature priorities.
- Historical proposals may contain old syntax and dependency sketches. Their
  introductory notices must make that clear; current priorities belong here.

Async/await, a JIT, and an interpreter remain out of scope, rather than waiting
at the bottom of this backlog.

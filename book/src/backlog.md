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

The remaining immediate order is:

B22's contract/runtime audit and atomic ownership foundation are complete.
Metadata is static, and counts synchronize last-release cleanup, including inline
frames. Concurrent stress tests cover the handoff. Public threads, effect/eligibility
checking, channels, and executors remain separate follow-on work under B22.

**Next: B10, recursive data types.** The [representation design](proposals/recursive-data.md)
uses existing heap records as indirection, preserves fallible constructors and
inline standard sums, and requires a compilation-owned nominal definition table.
Implement stable identities and component property analysis before accepting
source recursion. Validate allocation-free deep destruction; explicitly gate
automatic operations whose runtime traversal is still recursive. Recursive source
declarations remain unsupported until these pieces are connected.

Named function values already support basic callback registries: a
`list[Callable[[i64], i64]]` can hold named functions, including inside a class,
and `callbacks[index](value)` calls the selected function. These are indirect
calls with a fixed signature; captured environments are the missing storage
capability. B32 tracks that extension after B31, with lower urgency because this
baseline works. See [function values](design/25-function-values.md).
Explicit state beside a named callback also supports stateful registries today;
the [runnable lesson](tutorial/76-store-stateful-callbacks.md) and native tests
cover generic state, allocation-free dispatch, and exactly-once resource cleanup.

### Scope of the new priorities

**B30 — writable nested paths.** Begin with existing list elements and paths
through class fields; define dictionary-value updates separately from insertion.
An assignment to an existing scalar slot must not copy its containing collections
or allocate. Allocating the right-hand value or growing a container keeps its
ordinary fallible contract. Lower the destination as a writable storage location,
with precisely specified evaluation order and one evaluation of each expression.
Investigate scoped reborrows, delayed address resolution, and reservation of a
destination during evaluation. In particular, a call on the right-hand side must
not invalidate a saved element address by resizing or removing its parent.
Start with conservative loans over the enclosing collection; finer disjoint-index
reasoning belongs to B11. Acceptance includes allocation-disabled nested writes,
alias conflicts, side-effecting indices, invalid indices/missing keys, `?` exits,
and exactly-once destruction of replaced owners.

**B31 — fewer concepts to learn.** Audit `Closure`, `OnceClosure`, `Callable`,
`OnceCallable`, `def once`, and capture syntax using small callback, factory, and
owned-resource examples. Separate semantic requirements from their spellings:
reusable versus consuming calls affect ownership, and owned/shared/exclusive
captures reuse general borrowing rules, but that does not justify every exposed
name. Review inference, the dual role of `Callable` as a stored type and generic
constraint, and whether consuming behavior needs explicit syntax everywhere.
For each concept, retain it with a concrete reason, simplify it, or move it to a
library. Audit the functions-as-values lessons alongside the design; the basic
path should teach named callbacks and ordinary closures before advanced ownership
cases. Preserve explicit allocation, deterministic cleanup, useful diagnostics,
and bounded compilation. Decide this before B32 or executor APIs expand the surface.

**B22 — useful native concurrency.** Target native threads that execute Plenty
code in parallel without a global interpreter lock. First audit reference counts,
type metadata, allocators, destructors, and foreign-handle thread affinity;
moving a container can still leave shared string aliases behind. Define which
owners may transfer and which borrows may be shared. Then implement scoped
threads and bounded channels, followed by a Python-style thread-pool executor
with submission, result retrieval, ordered mapping, and context-managed shutdown.
Thread/task creation and queue allocation must report failures; specify bounded
queues/backpressure, failed-send ownership, joining on every exit, cooperative
cancellation, and partial-result cleanup. Executor result handles do not require
async/await. Build explicit Rayon-style parallel operations on these foundations;
automatic parallelization remains B24. Prefer library APIs over new syntax, with
compiler support for ownership checks and runtime support for native threading.
Scoped generic tasks need not wait for heterogeneous closure storage in B32.

**B32 — stored stateful callbacks.** After B31, compare storing one known concrete
environment type in generic containers/classes/enums with storing different
environments behind one callable signature. The former can use a known layout;
the latter needs an explicit representation and dispatch contract. Evaluate
fallible owned boxing and bounded inline storage against actual registry/event
handler needs before adding public types. Specify reusable/consuming invocation,
move/drop behavior, allocation failure, and capture lifetimes. Begin with owned
environments; escaping borrowed captures depend on B11. Keep Plenty callback
storage distinct from C ABI callbacks and foreign-library lifetimes in B28.

## Work items

The order above selects the next slices. This table is the catalog of open items,
including later candidates; its row order does not set priority. Detailed syntax
is still open.

| ID | Work | Scope / dependency |
| --- | --- | --- |
| B10 | Recursive data types | Define indirection, ownership, allocation failure, and bounded destruction for recursive classes/enums. Current acyclic forward declarations are already supported. [Sum-type design](proposals/sum-types.md). |
| B11 | Broader borrowing | Stored references, relationships among multiple reference parameters, and more precise collection loans. B30 takes the immediate nested-write subset without requiring this whole item. Evaluate precision against compile-time cost; the current single-parameter returned-borrow rule is documented in the [reference](design/18-returned-references.md). |
| B12 | Practical I/O and iteration | Fallible file iteration, explicit binary buffers and read/write, buffering, and reusable iterator/view protocols. Follow familiar Python conventions where they fit ownership and Result-based errors. |
| B13 | Public allocator control | Global/default and per-container selection, allocator state/lifetimes, and buffer provenance. Internal object allocator identity already exists. Consider bounded/inline-capacity storage separately. [Memory design](proposals/memory-parallelism-and-simd.md). |
| B14 | Typed error composition | Preserve details across multiple error types through explicit unions/conversions. `Failure` already supplies deliberate erasure; it does not replace recoverable typed errors. |
| B15 | Enum-pattern shorthand | Infer a user-defined enum's qualifier from the matched value, so arms need not repeat it. Define ambiguity and name-resolution rules. Catch-all `case _:` is already implemented. |
| B16 | Additional inference and value conveniences | Consider expected-result inference, partial explicit type arguments, and contextual literals across generic arguments. Standalone unit bindings/parameters/storage, nested unpacking, and slice syntax/steps also need concrete use cases and contracts. These remain candidates, not automatic extensions of today's inference rules. |
| B17 | Modules and packaging | Package/dependency configuration, separately cached module objects, re-exports, and import-cycle policy. Preserve absolute imports, visibility, and deterministic name resolution. [Module design](proposals/entrypoints-modules-and-tutorials.md). |
| B18 | Windows and additional targets | Target-specific runtime support, linker-driver conventions, object/executable naming, and native CI. Depends on target validation and packaging work; changing the linker alone is insufficient. |
| B19 | Measured codegen and storage improvements | Benchmark dense-match dispatch before adding jump tables. Investigate compact tuple/aggregate layouts, packed numeric buffers, code size, and compilation/link latency. Do not carry over assumptions from the legacy stack frontend. |
| B20 | Optional GCC backend | Evaluate the proposed backend with a small feasibility experiment before committing to another backend or common IR. Cranelift remains the current backend. [GCC proposal](proposals/codegen-gcc.md). |
| B21 | Documentation authoring | Evaluate independently runnable literate lesson sources that generate mdBook pages if they improve authoring. Current Markdown lessons are already executable and tested; do not create another manually maintained copy. [Documentation design](proposals/entrypoints-modules-and-tutorials.md). |
| B22 | Threads, channels, and parallel operations | Promoted: contract/runtime audit, then scoped threads and bounded channels, a Python-style thread-pool executor, and explicit Rayon-style parallel operations. Follow the staged scope above, including fallible submission and deterministic shutdown. [Concurrency design](proposals/memory-parallelism-and-simd.md). |
| B23 | SIMD | Define vector values, contiguous numeric storage, supported operations, and scalar fallbacks. Ordinary generic lists are not automatically packed native arrays. [SIMD design](proposals/memory-parallelism-and-simd.md). |
| B24 | Automatic parallelization | Research only after B22: require evidence that effects, cleanup, allocation failures, and result ordering remain correct. A runtime thread-count setting alone does not establish those guarantees. |
| B27 | Expanded C representations | Export text/byte buffers and borrowed views, broader typed errors and recoverable returned ownership, and explicit class-method adapters. Keep allocator provenance, output initialization, and error-path transfer visible. Initial class factories support AllocError and generated consumer wrappers require one fallible allocation. See [owned exports](design/24-c-interfaces/03-owned-exports.md). |
| B28 | Broader library loading and lifecycle | Generate runtime loaders directly from extracted contracts without original export source; improve error detail and binding tooling for third-party C libraries. Unloading, retained/foreign-thread callbacks, C record layout, and header-assisted bindings need separate contracts. Current loaders keep mappings resident and require their creating thread. [Runtime loading](design/24-c-interfaces/06-runtime-loading.md). |
| B29 | Further generic constraints | Consider protocol/callable bounds on data declarations, generic methods satisfying protocol requirements, combined bounds, and protocol composition when concrete library APIs need them. Generic aliases and enum constructor inference also remain candidates. Preserve explicit lookup and bounded specialization. See [generic data](design/28-generic-data-types.md) and [parameterized protocols](design/29-parameterized-protocols.md). |
| B32 | Stored stateful callbacks | Named-function registries already work. After B31, design owned captured-environment storage, separating homogeneous concrete layouts from heterogeneous dispatch. Borrowed storage depends on B11; concurrency eligibility is B22. See scope above. |

## Reconciled completed work

These old backlog entries are closed, not additional tasks. The linked reference
pages own the current details; this table records why they disappeared from the
active list.

| Previous item | Resolution |
| --- | --- |
| B09: Initial generic data types and richer protocols | Concrete generic classes/enums, IntType data bounds, function/constructor inference, inferred/explicit generic methods, parameterized structural contracts, and protocol-driven inference are implemented. Native allocation-failure/drop checks and specialization reuse/limits cover the initial subset. Extensions are B29; recursive storage is B10. |
| B08: Function values and multiline closures | Thin functions, reusable/consuming concrete environments, explicit captures, factories, signature inference, `Callable`/`OnceCallable` constraints, inline sums, captured generator frames, and checked lifecycle cleanup are implemented without implicit allocation. Completion records implementation, not final approval of the surface: simplification is B31 and broader captured-environment storage is B32. Stored/escaping borrows remain B11. See [closures](design/26-closures.md) and [consuming closures](design/27-consuming-closures.md). |
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

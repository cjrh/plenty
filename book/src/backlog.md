# Backlog

This is the single maintained list of planned language, compiler, runtime, and
tooling work. It replaces the separate roadmap and next-work lists. Entries are
candidates, not promises about syntax or delivery dates. Allocation-free ranges
and concrete generator frames are implemented independently.

Use the [implementation status](design/toolchain/02-architecture/support.md) to see what
works today, the [reference](design/index.md) for its exact contract, and the
[proposals](proposals/index.md) for design rationale and alternatives. A proposal's
stages describe technical dependencies, not a second priority list.

## Current position

Audited against local `main` at `5ab56b7` on 2026-10-11, including merged local
PRs and the shared issues register. B22's committed batch is complete. Work has
continued through B34 cleanup, B35 inline storage, and the initial sorting and
assignment operations. **B34 remains the current review phase**; completed
cleanup slices do not mean every recorded defect or design concern is resolved.

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
[reference](design/facilities/03-concurrency/threads.md) and
[runnable lesson](tutorial/11-concurrency/scoped-threads.md).

B10 is complete for the initial source subset: classes, enums, aliases, and finite
generic instances support recursive ownership through `Box` and collections.
Native tests cover construction, consuming matches, borrowing/moves, allocation
failure, allocation-free replacement, and deep drop. Imports preserve visibility; static and shared libraries preserve
opaque C handles and generated ownership wrappers. The
[runnable lesson](tutorial/09-generics/recursive-data.md) teaches chains and trees.
See the [implemented contract](design/language/03-types/recursive-data.md) for precise limits.

B11's borrowed enum matching slice is complete. Shared matches preserve owners;
mutable matches support inline sums and affine enum payloads. Payload loans protect
variants, support single-origin returns, and allocate nothing, including nested
`Option`/`Result` projections. Copyable enum storage remains immutable.
See the [runnable lesson](tutorial/06-data/08-borrowed-matching.md) and
[implemented contract](design/language/05-ownership/05-borrowed-matching.md).

**B22's committed concurrency batch is complete**, building on the
[thread-pool executor](design/facilities/03-concurrency/executors.md). Fixed workers,
bounded owned submission, recoverable jobs, affine futures, pending cancellation,
ordered list/range mapping, and context-managed drain/join are implemented.
Explicit [fallible mapping and fixed-tree reduction](design/facilities/03-concurrency/parallel-map.md)
are implemented too. The final three slices are implemented:

1. [Cooperative cancellation tokens](design/facilities/03-concurrency/cancellation.md) for running tasks.
2. [Timed channel operations and non-consuming future waits](design/facilities/03-concurrency/timed-selection.md).
3. Allocation-free receive selection between two typed channels.

Zero-capacity rendezvous, broader iterator/closure mapping, streaming results,
richer certified effects/types, and automatic parallelization (B24) are explicitly
deferred beyond this batch. Arbitrary-size and send-side selection are also
deferred; the initial receive pair keeps heterogeneous messages statically typed.
**Continue B34, cleanup and taking stock**, before starting unrelated feature work.
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
[stored closures](design/language/06-functions/04-consuming-and-stored-closures.md).
Explicit fallible [BoxedCallable](design/language/06-functions/05-boxed-callables.md)
now erases different owned shared-call environments behind one signature (issue #25).
Mutable, consuming, borrowed, generator-containing, and worker-bound erased modes
remain unsupported. See also
[function values](design/language/06-functions/02-function-values.md).
Explicit state beside a named callback also supports stateful registries today;
the [runnable lesson](tutorial/10-functions/explicit-state-callbacks.md) and native tests
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
and ordinary fallible container construction. Shared-call erasure now uses explicit
fallible boxes; broader erased modes and escaping borrowed environments remain
separate B32/B11 work. Generator-containing
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
and partial-result cleanup; see [parallel operations](design/facilities/03-concurrency/parallel-map.md).
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

Completed cleanup includes:

- Explicit handling of Result expressions (issue #15), reporting `main` errors
  and distinct `IoError` variants (issue #16), and complete entry-point and
  positional `open` diagnostics/documentation (issues #62 and #63).
- Inline classes, tuples, and enums (B35, issue #17); safe owned-field
  replacement, box method access, and heap-child destruction order (issues #4,
  #47, and #46).
- Reference cursors, signature-directed shared borrowing, precise tuple loans,
  and clearer borrow diagnostics (issues #1, #29, #51, #11, and #50).
- Checked tail-call transfers and cleanup, reference forwarding, and an explicit
  stack-overflow diagnostic (issues #3, #7, #8, #9, and #2).
- Linear checking in declarations and parallel native function emission (issues
  #53 and #55), runtime tests in CI, and thread-local allocation
  test markers (issues #48 and #14).
- Collection element access, hash-table removal, aggregate equality, and string
  indexing improvements (issues #18, #19, #20, and #21), plus correct parsing of
  nested index expressions (issue #23).
- Reorganized tutorial and reference, with verified proposal status blocks
  (issues #43 and #44; local PR #24).
- In-place sorting, element swapping, and assignment to several targets
  (issue #31); further ordering operations are B37.
- Callable mismatch guidance, closure source locations, and a shorter generic
  callback teaching path (issue #24; local PR #33). `Callable` and `Closure`
  retain their distinct representations and ownership rules.
- Direct borrowing of named functions, checked local function references, and
  tutorial examples without intermediate bindings (issue #26; local PR #36).

Continue addressing open issues in severity order. Concurrency API consistency
and join/deadlock diagnostics (issues #28 and #12) remain open. Claims and
current ownership belong in `issues.db`; these references do not assign work.
`Failure` still keeps no details; storing a cause in it belongs to B14.

Other cleanup findings include borrowing inline anonymous functions (issue #27),
source locations for runtime traps (issue #37), targeted diagnostics for Python
habits (issue #38), and receiver/parameter mutability consistency (issue #42).
Generic diagnostics belong to B29 (issue #40), and type-facts computation
to B19 (issue #6). Reassess the mutable-enum restriction in issue #41 against B35's
inline storage before choosing a fix; it remains open in the register.

The newer package-tooling agreement (issue #45) belongs to B17. The std catalogue
and finder prerequisites (issues #49, #60, and #61) are recorded under B38/B12.
They are future work, not shipped facilities or a reason to skip B34. The finder
experiment filters supplied paths; it does not enumerate directories.

## Work items

The current phase above selects the next work. This table catalogs remaining
work, including deferred extensions to completed batches; its row order does
not set priority. Detailed syntax is still open unless an agreement is cited.
Issue references identify related open concerns or completed fixes as stated;
they do not approve every suggested API. Status, reproductions, discussion, and
ownership remain in the shared `issues.db`.

| ID | Work | Scope / dependency |
| --- | --- | --- |
| B11 | Broader borrowing | Borrowed enum matching and payload loans complete. A `mut` reference binding is reassigned only through itself (issue #1). Remaining: stored references, relationships among multiple reference parameters, more precise collection loans, retargeting a reference binding to another borrow of the same owner, and field-precise loans through `mut` reference bindings. Related open concerns: borrowed iteration's value/reference rule (issue #30, also B12) and mutable matching of copyable enum payloads (issue #41; reassess after B35). B30 completed ordinary nested writes; disjoint-index precision remains here. Evaluate precision against compile-time cost; the current single-parameter returned-borrow rule is documented in the [reference](design/language/05-ownership/04-returned-references.md). |
| B12 | Practical I/O and iteration | Text-file iteration is implemented. Remaining: explicit binary buffers and read/write, buffering, and reusable iterator/view protocols. Define a consistent borrowed-iteration rule (issue #30, also B11); iteration helpers such as `enumerate` and `zip` are part of issue #35 (also B38). Lossless platform paths and binary argument/I/O transport are concrete gaps (issue #61); coordinate their representation with B38's filesystem work. Follow familiar Python conventions where they fit ownership and Result-based errors. |
| B13 | Public allocator control | Global/default and per-container selection, allocator state/lifetimes, and buffer provenance. Internal object allocator identity already exists. Consider bounded/inline-capacity storage separately. [Memory design](proposals/memory-parallelism-and-simd.md). |
| B14 | Typed error composition | Preserve details across multiple error types through explicit unions/conversions. `Failure` already supplies deliberate erasure; it does not replace recoverable typed errors. Option/Result conveniences and explicit conversion from Option to Result are related gaps in issue #35 (also B38). |
| B15 | Enum-pattern shorthand | Infer a user-defined enum's qualifier and generic arguments from the matched value, so arms need not repeat them (issue #36). That issue also proposes nested, or-, and literal patterns; assess those extensions separately. Define ambiguity and name-resolution rules. Catch-all `case _:` is already implemented. |
| B16 | Additional inference and value conveniences | Consider expected-result inference, partial explicit type arguments, and contextual literals across generic arguments. B35 completed inline storage (issue #17); classes still move, and copyable tuple storage still rejects mutable borrows. Changing those ownership rules is a separate candidate. Related gaps: augmented assignment with one evaluation of the target path (issue #32; use B30's nested-write machinery) and checked integer conversions/unsigned-negation rules (issue #39). Standalone unit bindings/parameters/storage, nested unpacking, and slice syntax/steps also need concrete use cases and contracts. These remain candidates, not automatic extensions of today's inference rules. |
| B17 | Modules, project tooling, and packaging | File imports and visibility are implemented; project tooling is not. The agreed direction in issue #45 separates the network-free `plentyc` compiler from the `plenty` project tool, reserves `std`, and adds a manifest, Git/path dependencies with repository subdirectories, import aliases, a committed lockfile, and debug/release profiles. Define reproducible artifacts (issue #58), separately cached module objects, re-exports, and import-cycle policy. Preserve deterministic name resolution and standalone compilation. Coordinate std distribution with B38. [Module design](proposals/entrypoints-modules-and-tutorials.md); issue #45 owns the newer package agreement. |
| B18 | Windows and additional targets | Target-specific runtime support, linker-driver conventions, object/executable naming, and native CI. Depends on target validation and packaging work; changing the linker alone is insufficient. |
| B19 | Measured codegen and storage improvements | Benchmark dense-match dispatch before adding jump tables. B35 completed inline storage; measure natural field packing and inline copies beyond the current 16-byte slots and runtime copy calls. Remaining concerns include full-row relocation during tail `list.pop()` (issue #52), repeated type-graph walks for collection facts (issue #6; measure before changing), packed numeric buffers, code size, and compilation/link latency. Collection access/removal, equality, and string indexing improvements (issues #18–#21), linear declaration checking (issue #53), and parallel native function emission without release verification (issue #55) are complete. Parsing/checking and IR construction remain serial (issue #59); very large functions have superlinear register allocation (issue #56), compiler peak memory is about 7.5 KB per source line (issue #54), and checking many small functions is mildly superlinear (issue #57). |
| B20 | Optional GCC backend | Evaluate the proposed backend with a small feasibility experiment before committing to another backend or common IR. Cranelift remains the current backend. [GCC proposal](proposals/codegen-gcc.md). |
| B21 | Documentation authoring | Evaluate independently runnable literate lesson sources that generate mdBook pages if they improve authoring. Current Markdown lessons are already executable and tested; do not create another manually maintained copy. [Documentation design](proposals/entrypoints-modules-and-tutorials.md). |
| B22 | Threads, channels, and parallel operations | Committed batch complete: eligibility/effect checks, scoped native threads, consuming jobs, bounded channels, executors, explicit fallible mapping/tree reduction, cancellation tokens, timed waits, and receive-pair selection. Open cleanup findings under B34: automatic-join hangs with an outside receiver (issue #12), the unreachable `SpawnError.PermissionDenied` variant (issue #13), and inconsistent task argument rules (issue #28). Broader mapping, richer effects, rendezvous, and broader selection are deferred beyond B34. Preserve fallible submission and deterministic shutdown. [Threads](design/facilities/03-concurrency/threads.md); [channels](design/facilities/03-concurrency/channels.md); [executor](design/facilities/03-concurrency/executors.md); [parallel operations](design/facilities/03-concurrency/parallel-map.md); [cancellation](design/facilities/03-concurrency/cancellation.md); [timed waits and selection](design/facilities/03-concurrency/timed-selection.md). |
| B23 | SIMD | Define vector values, contiguous numeric storage, supported operations, and scalar fallbacks. Ordinary generic lists are not automatically packed native arrays. [SIMD design](proposals/memory-parallelism-and-simd.md). |
| B24 | Automatic parallelization | Explicitly deferred beyond B34. Require evidence that effects, cleanup, allocation failures, and result ordering remain correct. A runtime thread-count setting alone does not establish those guarantees. |
| B27 | Expanded C representations | Export text/byte buffers and borrowed views, broader typed errors and recoverable returned ownership, and explicit class-method adapters. Keep allocator provenance, output initialization, and error-path transfer visible. Initial class factories support AllocError, which reports allocation of the boxed handle. See [owned exports](design/facilities/04-interop/owned-exports.md). |
| B28 | Broader library loading and lifecycle | Generate runtime loaders directly from extracted contracts without original export source; improve error detail and binding tooling for third-party C libraries. Unloading, retained/foreign-thread callbacks, C record layout, and header-assisted bindings need separate contracts. Current loaders keep mappings resident and require their creating thread. [Runtime loading](design/facilities/04-interop/loading.md). |
| B29 | Further generic constraints | Callable bounds on data are implemented. Consider protocol bounds on data declarations, generic methods satisfying protocol requirements, combined bounds, and protocol composition when concrete library APIs need them. Generic aliases and enum constructor inference also remain candidates. Related diagnostic: report instantiation failures at the call site with body context (issue #40). Preserve explicit lookup and bounded specialization. See [generic data](design/language/07-generics/data.md) and [parameterized protocols](design/language/07-generics/protocols.md). |
| B32 | Broader callback storage | Owned concrete reusable/consuming environments and their worker eligibility are implemented. Explicit fallible shared-call erasure is implemented (issue #25). Remaining: mutable/consuming erased modes and generator-containing environments in heap owners. Escaping borrowed storage depends on B11. See scope above. |
| B33 | Deep operations on recursive values | Recursive source storage is implemented under B10. Replace native recursive copy/format traversals with iterative fallible work storage before enabling them; select an explicit bounded-memory contract for structural equality and membership without adding hidden allocation failure. Preserve failure cleanup, shared-string ownership, and float/NaN semantics. [Design rationale](proposals/recursive-data.md). |
| B34 | Post-concurrency cleanup and review | Current phase, partly complete. Result handling, ownership/tail-call fixes, performance improvements, test reliability, and book restructuring have landed. Callable diagnostics now explain code-only parameters and generic callback ownership, identify closures by source location, and retain distinct `Callable`/`Closure` representations (issue #24). Direct borrowing of named functions and its tutorial updates are implemented (issue #26). Continue auditing code and docs and resolving open issues in severity order. Borrowing inline anonymous functions (issue #27), runtime trap locations (issue #37), Python-habit diagnostics (issue #38), and receiver/parameter mutability (issue #42) remain open. Concurrency findings are linked under B22; generic diagnostics under B29. See scope above; the completed documentation reorganization does not close all review findings. |
| B36 | Keyword arguments, defaults, and static unpacking | Keyword/default/variadic arguments and starred unpacking are unimplemented; ordinary tuple bindings and assignment to several targets already work. Proposed: keyword and keyword-only arguments, constant defaults, `/` and `*` markers, keyword constructors, and `*`/`**` unpacking at calls, in displays, and in tuple bindings. Call binding and tuple arities must resolve at compile time, with no runtime arity/key check or hidden allocation; collection displays retain explicit fallible allocation. Keyword arguments and defaults are frontend-only and come first; `*args` and sequence patterns depend on a borrowed sequence view (B12). Decide whether `first, *rest = tuple` is accepted now that tuples are inline. [Design](proposals/keyword-arguments-and-unpacking.md). |
| B37 | Ordering beyond `sort()` | `list.sort()` orders integers, floats, `str`, and tuples of those in place; `swap` and assignment to several targets are complete (issue #31). `reverse()` after `sort()` already gives descending order. Remaining (issue #64): key-function sorting, a direct descending-order option, and a `sorted` expression. Decide stability, how a key reads an element it does not own, and whether key storage makes the operation fallible. Define `<` on `str` (issue #22) and on tuples to match `sort`, then let a class opt in to ordering. Preserve no hidden allocation. |
| B38 | Standard library and filesystem foundations | The catalogue in issue #49 proposes coherent `std` module families and a production-grade HTTP server, with ordinary library code in Plenty. It is not implemented, and its individual APIs need contracts and acceptance checks. Related foundations: string character codes/classification and whitespace splitting (issue #22; ordering is B37), integer bitwise/wrapping operations (issue #33), math/numeric helpers and exponentiation (issue #34), and small iteration/container/Option helpers, tuple keys, and a deque (issue #35, also B12/B14). Separate library APIs from language prerequisites; checked conversions are B16 (issue #39). Start by identifying prerequisites: byte buffers and lossless platform paths/transport (B12, issue #61), directory enumeration and file-type metadata (issue #60), and standard-library distribution/package boundaries (B17, issue #45). Networking/HTTP needs explicit ownership, concurrency, partial-I/O, cancellation, and resource limits. The finder experiment supplies evidence about gaps, not a filesystem implementation. |

## Reconciled completed work

These old backlog entries are closed, not additional tasks. The linked reference
pages own the current details; this table records why they disappeared from the
active list.

| Previous item | Resolution |
| --- | --- |
| B35: Inline values | Classes, tuples, and enums store their fields inline; their storage construction allocates nothing and needs no `?` (issue #17). Argument expressions and custom initializers retain their own failures, so a fallible initializer still makes its constructor return Result. Recursive types use `Box[T]`, which converts to its content wherever the content's type is required; recursion without a heap boundary is rejected. C export handles box their instance. See [classes](design/language/03-types/classes.md) and [recursive data](design/language/03-types/recursive-data.md). |
| B10: Recursive source types | Self/mutual recursive classes and enums, aliases, and finite generic instances reach themselves through boxes and collections (originally heap records; see B35). Finite compiler graphs avoid type-table leaks; native source tests drop 100,000-node class and enum chains without allocation on a 256 KiB stack. Runnable lessons, module visibility, and C/Plenty library consumers cover the initial subset. Borrowed matching is B11; automatic deep operations are B33. |
| B09: Initial generic data types and richer protocols | Concrete generic classes/enums, IntType data bounds, function/constructor inference, inferred/explicit generic methods, parameterized structural contracts, and protocol-driven inference are implemented. Native allocation-failure/drop checks and specialization reuse/limits cover the initial subset. Extensions are B29; recursive storage was completed under B10. |
| B08: Function values and multiline closures | Thin functions, reusable/consuming concrete environments, explicit captures, factories, signature inference, `Callable`/`OnceCallable` constraints, inline sums, captured generator frames, and checked lifecycle cleanup are implemented without implicit allocation. B31 reviewed the surface and simplified its teaching path; captured-environment storage remains B32 and stored/escaping borrows remain B11. See [closures](design/language/06-functions/03-closures.md) and [consuming closures](design/language/06-functions/04-consuming-and-stored-closures.md). |
| B30: Nested assignment | Existing list/dictionary/field slots update without enclosing copies or allocations. RHS and saved indices precede address resolution; tests cover resizing, aliases, failure cleanup, and owner replacement. |
| B31: Callable simplicity audit | The initial review recorded use-case rationale and a simpler teaching path; ordinary generic records demonstrate stateful registries without another callable type. The follow-up agreed to keep `Callable` and `Closure`, explain mismatches through generic callback guidance, and display closure source locations (issue #24 under B34). |
| B07: Initial runtime library loading | CLI/API generation emits self-contained typed loaders; exact discovery metadata and all symbols are checked before returning a table. Scalar/borrow/Result adapters and owned factories/transfers preserve signatures, destruction, and instance provenance. Allocation-free LoadError, injected failures, private-address checks, and the executable library tutorial cover the initial resident implementation. [Runtime reference](design/facilities/04-interop/loading.md). |
| B06: Initial C library exports | Static/shared packaging, scalar and Result adapters, owned factories, matching destruction, shared/mutable and consuming handles, precise C contracts, embedded/extractable interfaces, SHA-256 link guards, explicit binary verification, and staged publication are implemented. Native callers and injected failures validate cleanup. [Library reference](design/facilities/04-interop/exports.md). |
| B05: Ownership-aware C adapters | Scalar/pointer borrows and explicit UTF-8/C-string adapters integrate with normal borrowing. Private opaque pointers in classes provide owned handles and matching destruction; C fixtures verify partial acquisition, allocation errors, and conditional/unconditional transfer. Mutable/returned byte buffers remain outside this subset. See [adapters](design/facilities/04-interop/borrowing-text.md). |
| B04: Initial C imports | Trusted `.plentyi` declarations support sized C scalars and nominal opaque pointers. Native C fixtures cover static/shared linking and actual ABI calls; visibility and Plenty layouts remain separate. See [C interfaces](design/facilities/04-interop/trusted-declarations.md). |
| B03: Target compatibility | Native compilation validates the supported x86_64 Linux GNU target, packaged runtime triple, and ISA pointer width. Explicit target selection rejects incompatible layouts before source loading/emission; runtime extraction includes target metadata. See [runtime packaging](design/toolchain/02-architecture/runtime.md). |
| B02: Object-file output | CLI/API application object emission and matching runtime extraction work without a linker on PATH. External-driver tests link and run single-file and imported programs. See [execution commands](design/toolchain/01-building/commands.md). |
| B01: Linker selection | CLI `--linker` / `--link-arg` and Rust `CompileOptions` select a cc-compatible driver without shell parsing. Native and mock-driver tests cover argument fidelity, failure diagnostics, and temporary cleanup. See [execution commands](design/toolchain/01-building/commands.md). |
| B25: Allocation-free generator frames | Generator calls return concrete inline frames directly. Consumers specialize by producer identity; factories and standard sums preserve that identity. Native tests disable heap allocation across creation, moves, calls, nested frames, resume, and cleanup. Recursive inline layouts are rejected. See [native generators](design/language/09-iteration/03-generators.md). |
| B26: Allocation-free ranges | Range construction now returns a copyable inline value directly. Calls, returns, standard sums, indexing, membership, and repeated traversal need no range allocation. Stored ranges live in their containing owner's storage. See [collections and iteration](design/facilities/01-builtins/collections.md). |
| String/heap reclamation | Owned values and strings are reclaimed by the Rust runtime, with deterministic cleanup. See [ownership](design/language/05-ownership/index.md) and [destruction](design/language/05-ownership/06-destruction.md). |
| Length-aware strings | Strings already store explicit lengths and support embedded NULs. The original `strlen`-based premise is obsolete; adopting a two-word source value is not required to resolve it. |
| Precompiled runtime archive | The runtime archive is built with the compiler and embedded for linking. It is not rebuilt from C source for each program. See [runtime](runtime.md). |
| Foundation batch | Main/imports/visibility, fallible allocation APIs, borrowing, tuples/items, typed ranges, generic functions, and structural protocols have implemented paths. See the [current status](design/toolchain/02-architecture/support.md). |
| Allocation syntax and error convenience | Allocating operations return Result by default; `?`, explicit success values, and `Failure` replace the old `try`/`try_` alternatives and repetitive unwraps. |
| Generic argument inference | Calls infer concrete type parameters from arguments; explicit arguments remain available. See [generic functions](design/language/07-generics/index.md). |
| Catch-all matching | `case _:` already handles remaining enum variants. See the [enum lesson](tutorial/06-data/06-enums.md). |
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
- Use the [development workflow](design/../development/workflow.md) for shared testing,
  documentation, and performance expectations. Those are ongoing practices,
  not separate feature priorities.
- Historical proposals may contain old syntax and dependency sketches. Their
  introductory notices must make that clear; current priorities belong here.

Async/await, a JIT, and an interpreter remain out of scope, rather than waiting
at the bottom of this backlog.

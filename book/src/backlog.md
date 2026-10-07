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

The initial toolchain and C boundary batch (B01–B06) is complete. B07 is active:
typed runtime loading of known interfaces with recoverable loading/compatibility
errors, keeping loaded libraries resident. Build on the generated contracts,
fingerprints, and ownership wrappers already checked by native C/C++ and Plenty
integration tests. Expanded C representations are tracked separately as B27.
Trusted indirect C calls are implemented as the first runtime-loading building
block. The runtime now provides resident mappings and fallible lookup leases;
fixed ABI-checked declarations now expose those helpers to trusted interfaces.
Exact discovery-contract validation now returns recoverable errors. Generated
typed loading is still in progress.

## Later candidates

These entries are not ordered within the table. Dependencies and design links
guide breakdown when an item is selected; detailed syntax is still open.

| ID | Work | Scope / dependency |
| --- | --- | --- |
| B07 | Runtime library loading | Typed loading of known interfaces with recoverable missing-library/symbol and incompatible-contract errors; validate generated interface metadata through versioned discovery. Initially keep loaded libraries resident; unloading, retained callbacks, foreign-thread callbacks, C record layout, and header-assisted bindings need further contracts. [FFI design](proposals/ffi-and-dynamic-libraries.md) and [export contracts](proposals/ffi-export-contracts.md). |
| B08 | Function values and multiline closures | First-class named functions and callable signatures, then anonymous bodies and checked captures. Define borrowed versus owned environments, lifetime/escape rules, and fallible allocation before adding higher-order library APIs. |
| B09 | Generic data types and richer protocols | Generic classes/enums and methods, with parameterized protocols or additional bounds when concrete library use cases justify them. Keep method lookup explicit and measure specialization cost. [Protocol design](proposals/protocols-generics-and-context-managers.md). |
| B10 | Recursive data types | Define indirection, ownership, allocation failure, and bounded destruction for recursive classes/enums. Current acyclic forward declarations are already supported. [Sum-type design](proposals/sum-types.md). |
| B11 | Broader borrowing | Stored references, relationships among multiple reference parameters, and more precise collection loans. Evaluate precision against compile-time cost; the current single-parameter returned-borrow rule is documented in the [reference](design/18-returned-references.md). |
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
| B22 | Threads, channels, and parallel operations | Establish transfer/share rules, compatible reference counting, allocator and foreign-handle constraints; then scoped threads, bounded channels, and explicit parallel map/collect/reduce with defined failure and cancellation behavior. [Concurrency design](proposals/memory-parallelism-and-simd.md). |
| B23 | SIMD | Define vector values, contiguous numeric storage, supported operations, and scalar fallbacks. Ordinary generic lists are not automatically packed native arrays. [SIMD design](proposals/memory-parallelism-and-simd.md). |
| B24 | Automatic parallelization | Research only after B22: require evidence that effects, cleanup, allocation failures, and result ordering remain correct. A runtime thread-count setting alone does not establish those guarantees. |
| B27 | Expanded C representations | Export text/byte buffers and borrowed views, broader typed errors and recoverable returned ownership, and explicit class-method adapters. Keep allocator provenance, output initialization, and error-path transfer visible. Initial class factories support AllocError and generated consumer wrappers require one fallible allocation. See [owned exports](design/24-c-interfaces/03-owned-exports.md). |

## Reconciled completed work

These old backlog entries are closed, not additional tasks. The linked reference
pages own the current details; this table records why they disappeared from the
active list.

| Previous item | Resolution |
| --- | --- |
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

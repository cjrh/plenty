# Completed language and tooling work

[Completed-work index](../completed-work.md) · [Remaining work](../backlog.md)

These records describe delivered subsets and concluded decisions. References
own current contracts; extensions of a mixed B item stay in the backlog.

## Unicode trie example

The [runnable trie](../design/examples/data-structures/trie.md) stores exact
Unicode strings, including the empty key, with insertion, lookup, pruning
deletion, sorted prefix enumeration, and longest-prefix matching. Private
parent/sibling IDs support iterative operations and slot reuse. Model/invariant
checks cover permutation pairs, Unicode mixed histories and 256-way sibling
lists. Retained-result checks drop original key/query/trie owners before reuse
pressure and comparison. Allocation-failure sweeps cover insertion and prefix
results, and a Linux small-stack test handles
a 100,000-scalar key. This is a teaching set, not a `std` API, compressed trie,
or normalization policy. B73's general arena qualification remains outstanding.

## Balanced-tree examples

[Executable B-tree and left-leaning red-black sets](../design/examples/data-structures/balanced-trees.md)
implement integer-key insertion, lookup, deletion, ordered traversal and private
slot reuse directly in Plenty. A sorted-list model and independent structural
validators check exhaustive five-key permutation pairs, multi-level trees and
mixed histories. Runtime allocation-failure sweeps check preservation and retry;
deletion and reuse are checked with allocation disabled. These are educational
examples, not new `std` APIs or storage-engine performance qualification.
B73's general arena identity/deletion qualification remains outstanding.

## Linked-list tutorial

The [linked-list lesson](../tutorial/04-collections/linked-lists.md) demonstrates
owned singly linked push/pop and mutable traversal, then index-based doubly
linked append and bidirectional traversal. Executable examples preserve existing
links on allocation failure. Deletion, slot reuse and public handle validation
are explicitly outside this teaching implementation; B73 remains unchanged.

## Documentation interpolation colours

The default Ayu documentation theme colours f-string interpolation expressions
and their braces cyan, distinct from surrounding string text. Nested tokens
retain their syntax colours; other themes are unchanged.

## B11: Borrowing

Borrowed enum matching/payload loans, reference rebinding within the original
place, precise first-reference return origins and disjoint literal-index loans
landed (issues #1/#30/#41, local PRs #488/#490). Optional/fallible returned
references (`Option[&T]`/`Result[&T, E]`) shipped as phase 1 (issue #520,
local PR #622). `dict.get_mut` replaced dictionary indexed assignment
(issue #580, local PR #696); heap and graph reference getters shipped
(local PR #629).

Local dictionary key/value views (issue #396), list/byte slice views
(issue #581), mutable Bytes loops (issue #594), proven disjoint slice/loop-field
loans (issue #593) and scalar-indexed `&substr` substring views (issue #595)
are implemented. Methods on owned temporaries can lend within the statement
(issue #597), calls returning mutable references can be assigned through
(issue #598), and method receivers projected from temporaries borrow in place
(issue #599). `len` also normalizes explicit shared borrows of literals and
owned temporaries, preserving full-expression cleanup without allocating for
the borrow itself (issue #683).

See [borrowing](../design/language/05-ownership/02-borrowing.md),
[returned references](../design/language/05-ownership/04-returned-references.md),
[optional references](../design/language/05-ownership/05-optional-references.md)
and [slices/views](../design/language/04-expressions/slices.md).
Outstanding relationships remain **B11** in the [backlog](../backlog.md).

## B49: Length method dispatch

`len(value)` calls an accessible ordinary shared `len(self) -> i64` method on
classes and enums, including standard `Deque`, `Bytes`, and heap containers
(issue #623). Owners, references, and temporaries follow ordinary method
borrowing and cleanup. Unsupported length and membership diagnostics name the
actual operand type rather than claiming every iterable supports the operation.
See the [collection contracts](../design/facilities/01-builtins/collections.md).
User-container membership and iterator protocols remain **B49** in the
[backlog](../backlog.md); Python `__len__` and `__contains__` names remain reserved.

## B12: I/O and iteration

Text-file iteration, uniform borrowed iteration and borrowed temporary source
lifetimes shipped (issues #30/#75). Allocation-free enumerate/zip/reversed
sources and boolean reductions (issue #35) now compose through B75 pipelines;
free `any`/`all` were removed (local PR #562). Binary buffering is B39/B40;
lossless Unix arguments/binary I/O (issue #61), native Linux `std.path` and
local dictionary views are delivered. The finder exercises NUL-separated input
and output with `--null`; directory traversal/metadata shipped for issue #60.

See [iteration](../design/language/09-iteration/01-iterables.md) and
[native paths](../design/stdlib/path.md). **B12** in the
[backlog](../backlog.md) owns reusable/escaping views and portable paths.

## B14: Errors

[General anonymous sums](../design/language/03-types/anonymous-sums.md) preserve
payloads, normalize membership and support injection/widening. Precise errors,
`?` and explicit success values replaced the former `try`/`try_` allocation
conventions; `Failure` is a last resort (issue #342), not the recommended default.
`Option.unwrap_or` and `ok_or` shipped as part of issue #35. B71 records typed
presentation and `Result.map_error`/variant mapper values (issue #556).

[Arithmetic hooks](../design/language/04-expressions/operators.md) shipped for
classes/enums, including reflected and explicit in-place methods (issue #512).
One-field `@newtype` forwarding preserves nominal typing and explicit-hook
precedence (issue #555). [`checked:` blocks](../design/language/03-types/numbers.md#checked-blocks)
return typed integer overflow/zero-division errors for arithmetic written in the
block; float `div_checked` and nontrapping `MIN % -1` accompanied them
(issue #557). The lexical private callback channel from issue #614 (B93)
is superseded by independent declared function/closure error contracts in the
[checked-default compiler core](../proposals/checked-by-default/implementation.md#current-implementation).
That core also supports lexical trapping `unchecked:` suites. The private
callback channel is removed. Initial std migration covers math helpers, bytes
and time: integer reductions preserve typed overflow payloads, separate float
helpers retain infallible IEEE values, serial pipeline sums return public
results, and key sorting transports fallible callbacks' ordinary errors.
Encoding sizes, stream extents, formatting counters, filesystem budgets and
entropy-source offsets now preserve their existing error contracts or use
bounds-proven opt-outs. Filesystem walks reserve only newly appended frames
(issue #702); an allocation-failure regression distinguishes failure before
reservation from failure while scanning a popped sibling. Coordinated adoption
of remaining std modules, tests/examples and documentation is **not complete**
and remains B14.

Core HTTP decimal/chunk sizes, networking/readiness, bounded text operations,
deterministic random helpers and testing streams are migrated. HTTP accepts the
full representable Content-Length range without overflowing an ASCII-digit
intermediate (issue #703); larger lengths remain protocol errors. HTTP and DNS
reply-buffer extents report existing allocation-capacity errors before consuming
another chunk. `FakeStream.fail_after` now returns a precise overflow result and
preserves the previous schedule on failure. Counter exhaustion returns stream
OutOfRange before mutation. Representation-limit probes exercise the actual
private helpers without huge allocations, and RNG golden/reference sequences,
text-wrap compatibility/allocation tests and testing-toolchain checks pass.

URL output sizing, process request/output extents and incremental HTTP decoder
arithmetic are migrated. URL expansion checks its final representation before
allocation; terminal scans stop at the input endpoint. Process argument encoding
reserves only additional bytes (issue #704), with zero-allocation spare-capacity
and failed-mutation tests in debug and optimized output. Existing allocation
causes remain intact. The process and readiness suites pass; incremental decoder
parity tests pass apart from the still-unmigrated actor integration.

Typed HTTP framing and client arithmetic now preserve existing typed transport
causes and progress. Complete chunk wire extents are checked before delivery;
representation failure is OutOfRange, separate from invalid transport counts.
Boundary tests use an affine cause type without constructing huge buffers and
verify allocation-free arithmetic in debug and optimized output. Client
deadline saturation, bounded counters and callback independence are explicit.
The I/O range integration and typed composition/prototype examples pass;
those cross-module dependencies were resolved by the coordinated checkpoint
below.

JSON position arithmetic now reports the existing InputTooLarge fault before
advancing a failing byte or ASCII-run cursor; input/output budgets and partial
output progress remain intact. Unicode normalization arithmetic has explicit
scalar-ceiling, successful-append and positive-index bounds. Both public APIs
are unchanged. The component's 35 runtime-check tests pass on an isolated build
target, including optimized/unoptimized boundaries and the Unicode normalization
corpus; independent review found no defects. A separately reviewed TOML/regex
checkpoint adds 40 passing component tests and preserves parse errors, document
edit atomicity, regex work/memory limits and replacement error precedence.
Public Match lengths now expose typed subtraction overflow; TOML location
exhaustion uses InputTooLarge with unavailable zero coordinates at the valid
offset. XML/CSV and broader adoption remain outstanding.

The coordinated numerical, transport, server and actor checkpoint is integrated
with independent source reviews. Ordinary integer SIMD operations return
lane-typed overflow results; floats and explicit wrapping operations retain
their contracts. Complex powers cover the full signed exponent range; matrix
construction/indexing and statistics keep their typed errors. Diagnostic logger,
server-report and actor counters saturate under the user-approved policy.
Exact streamed progress, allocation extents, crypto/DER/UUID and JWT time bounds
retain existing errors rather than hiding overflow.

Actor identity generations never wrap: exhausted slots and clock records retire.
Mailbox rebasing preserves every target-owned child-exit fence and prefix order;
affine callback arithmetic errors reach the parent without leaking actor heaps.
Combined targeted runtime-check validation reports 733 passes, two failures in
still-unmigrated HTTP body/cookie dependencies and one pre-existing ignored test.
Full crypto, JWT, HTTPS, standalone server/client and actor TLS tests now pass
together. The optional native-engine linalg bodies were initially unrun;
broad documentation examples, remaining modules and full acceptance measurements
remain outstanding.

Reviewed CLI/progress arithmetic now preserves exact work counts and output-failure
state; the countbytes example exposes its aggregate overflow contract. Dataframe
extents retain allocation-capacity causes, and plot counts/layout/rendering have
explicit bounds without changing numerical aggregates. All 48 plot tests pass;
the initial 13 dataframe native-engine self-skips were later replaced by real
engine execution: all 17 dataframe and nine linalg tests pass, including native
relocation and allocation cleanup after fixture-only arithmetic fixes.
HTTP body/cookie helpers preserve semantic TooLarge errors and multipart/upload
ownership. Actor HTTP has equivalent bounded lingering state without clamping
actual consumed bytes; its 26 TCP/TLS/control/error tests pass. Remaining HTTP
core failures still reach websocket. A reviewed proxy/helper checkpoint restores
all 19 helper tests, preserving descriptor/cancellation/RFC/bulk-scan assertions.
XML/CSV arithmetic preserves exact positions and explicit pending-output flushing
at representation exhaustion. Temporary/system/watch/SQLite wrappers and
compression retain their domain errors and resource/progress contracts; real
procfs/inotify/SQLite and gzip/Brotli interop tests run. Actor HTTP interop retains
curl and all 600 corpus mutations. Optional sqlite-vec and Docker cases remain
unrun at this checkpoint.

The final reviewed std arithmetic checkpoints cover archive, HTTP files/SSE/
WebSocket, MIME/template/markdown, sketches and diff/image. Public mutable diff
lengths return overflow Results. File metadata above i64.MAX is rejected before
conditional validators (issue #706); all 11 file adapter tests now pass after
MIME integration. The combined final component selection has 143 scoped passes,
followed by image 23, Markdown 10, math 23 and registry/index 2 passes. The index
reports no remaining diagnosed module. New byte-ring PNG filter operations are
explicitly wrapping; parser columns, numerical sketch totals and semantic output
progress remain checked rather than saturated. Independent review prompted a
Markdown source-comment correction and an image OOM fixture change to exercise
conversion rather than copying; both were applied and validated.
These are focused checkpoints, not full-suite/native/export/performance acceptance.
Reviewed parallel integer sums now expose precise first-overflow Results after
workers drain, preserving serial-prefix behavior and callbacks-once semantics.
All 30 pipeline tests pass. The initial implementation retains O(n) numeric values
and combines them serially; no performance improvement is claimed. Generated
split/count helpers have source bounds and exact extent errors; enumeration keeps
its explicit trapping boundary.
Reviewed early/advanced lessons, reference and skill examples use public callback
Results and explicit worker error handling. Combined executable documentation now
has 11 passing / 9 failing tests rather than 3 / 17. Unmigrated generic deque,
heap, graph and LRU instances plus fallible reducers remain actual blockers;
registry success does not cover them. Repository fixtures/examples, full native
and all-features acceptance remain outstanding.
Separate pre-existing archive-decimal and PNG-framing limits are recorded
as issues #705 and #707, open and outside these arithmetic checkpoints.

Class match patterns shipped with issue #558.
Outstanding conveniences stay **B14/B71** in the [backlog](../backlog.md).

## B16: Values and syntax

Block `if`/`elif`/`else` supports whole-RHS bindings, single-place assignments,
and explicit returns with the same contextual types, cleanup and lifetime
restrictions as match-valued statements (issue #674). Arbitrary expression
nesting remains intentionally unsupported; see
[conditional values](../design/language/04-expressions/branches.md#an-if-as-a-statements-value).

Tuple component borrows (issue #68), once-only numeric augmented-assignment
index evaluation (issue #32), checked casts/explicit wrapping conversions
(issues #39/#65), `RangeError` conversions and unsigned-negation checks
(issues #66/#67) shipped. Copying [slice syntax](../design/language/04-expressions/slices.md)
and `NonZero[i64]` range/slice steps shipped (issues #579/#584, local PR #698).
Negative literal tuple indices are supported (issue #583); nested `Shared`
handles gain owners in copies/slices (issue #582).

Hex/octal/binary integer literals and `\u{...}` string escapes shipped
(issues #563/#564). `@redacted` classes/enums format as `TypeName(@redacted)`
through automatic paths, including std secret-bearing types (issue #586).
`Bytes` now prints a round-tripping byte literal (issue #596).
See [numbers](../design/language/03-types/numbers.md),
[text escapes](../design/facilities/01-builtins/text.md#escapes) and
[redacted types](../design/language/03-types/classes.md#redacted-types).
Recursive tuple match patterns and nested tuple unpacking in declarations,
assignments, loops, comprehensions and generator-expression targets shipped
(issue #675). Product coverage,
affine cleanup and borrowed component loans use the existing pattern and tuple
machinery. Guards, general aliases and starred targets remain unsupported; see
[matching](../design/language/04-expressions/matching.md#tuple-patterns) and
[tuples](../design/language/03-types/tuples.md).

Remaining inference/value candidates stay **B16** in the [backlog](../backlog.md).

## B17: Tooling and packaging

[`plenty script`](../design/toolchain/01-building/scripts.md) delivers executable
shebang scripts, lossless Unix argument forwarding, strict optional embedded
manifests, isolated path/Git resolution, cached and durable locks, and original-source
diagnostics. Per-script private cache state and resolution locks keep generated files
out of source directories; warm runs reuse objects and eligible Linux executables.
Check/compile modes, editor header handling and cache inspection/cleanup accompany
the runner. A script test command and broader executable-cache platforms remain
outside this delivery. The [completed proposal](../proposals/completed/single-file-scripts.md)
records the rationale (local PR #851).

File imports/visibility, reserved embedded `std`, the `plentyc`/`plenty` split,
strict manifests, pinned Git/path dependencies, locks/offline mode, workspaces,
project commands/tests, per-target build reuse and `--package-graph` shipped
(issue #45, local PRs #124/#130). Native artifacts use a fixed baseline,
deterministic archives and runtime path remapping (issue #58). Release archives
and distribution smoke tests are established (issue #151, local PR #187);
[releasing](../development/releasing.md) owns the current workflow.

Default stripping/`--no-strip` (issue #500, local PR #575), removal of linked
Rust panic/backtrace code (issue #503, local PR #576), executable size budgets
(issue #502, local PR #580) and reachability-rooted destructors
(issue #506, local PR #581) reduce unused native linkage. Release/no-strip
option ordering is fixed (issue #590, local PR #732).

Single-file compile-and-run now caches linked executables by default on
supported Linux toolchains, including hashbang launches. Native dependency
and toolchain identities invalidate reuse; entries share cache inspection,
cleanup and eviction. Linker wrappers, including GCC-private overrides, are
excluded before executable lookup (issue #676). Wrapper regressions isolate GCC's
private search paths and verify linker selection, including the manylinux GCC
toolchain that prefers its private `ld` over PATH (issue #679). Explicit build
outputs, project runs, custom linkers, non-Linux hosts and instrumented builds
still link normally.
See the
[executable cache contract](../design/toolchain/02-architecture/cache.md#single-file-executables).

The [language server](../design/toolchain/01-building/language-server.md)
provides diagnostics, hover, definition, completion and simple-local inferred
type hints, with Neovim integration (issues #155/#554, local PR #196).
Compile-time constants/static assertions, embedded data and `if const target.*`
shipped (local PR #628). Module constant indexing and import/module diagnostics
were improved (issues #562/#568/#587).

The local [semantic-search trial](../development/semantic-search.md) has
repository extraction, incremental synchronization/deletion detection and hybrid
queries in Plenty; a pinned Python/FastEmbed subprocess performs inference.
[Packages](../design/toolchain/01-building/packages.md) and
[linking](../design/toolchain/01-building/linking.md) own current contracts.
Outstanding tooling/investigation scope stays **B17** in the [backlog](../backlog.md).

Engine-notices packaging reads the Plenty version directly from the root
`Cargo.toml` with Python's TOML parser, avoiding the nested Bash version lookup
that failed in Windows release CI (issue #687). Regression fixtures cover release
and prerelease versions, LF/CRLF manifests, unrelated version keys and a repository
path with spaces/Unicode. These local fixture checks do not substitute for a
native Windows release build.

Generated standard-library foreign wrappers retain their loader provenance for
unused-definition pruning (issue #688). Bare unused `std.dataframe`/`std.linalg`
imports emit no engine symbols in debug or optimized objects; required native
calls retain ordinary engine discovery. This fixes the Windows release smoke
failure after packaging; native Windows CI must still confirm the archive run.
Windows executable emission also omits unreachable user helpers and generators
from its temporary COFF object (issue #690), eliminating native imports only
those bodies require. Explicit objects, library objects and dumps retain their
inspection contract; missing reached imports remain linker errors. Actual COFF
links cover the unused-helper case and a reached-call negative control, in
both optimization modes. Native Windows execution remains a CI qualification
requirement. See the [emission policy](../design/toolchain/02-architecture/emission.md).

Distribution output appends the native executable suffix to the source stem
(issue #697), producing `analysis.exe` on Windows while preserving dotted stems
and Unix names. Filename regression tests cover both suffix conventions; the
existing native archive smoke test still checks dataframe distribution and
relocation. Native Windows execution remains a CI qualification requirement.

## B21: Documentation authoring

Executable Markdown checked by `tests/test_tutorial.rs` remains the lesson
source; generated literate lessons were evaluated and not adopted
(local PR #537). See the [documentation design](../proposals/completed/entrypoints-modules-and-tutorials.md).
This is a concluded decision, not an implementation waiting in the backlog.

## B22: Native concurrency

Scoped native threads, structural eligibility/reachable effect checks, consuming
jobs with failed-start ownership, bounded MPMC channels, executors, ordered
fallible map/tree reduction, cooperative cancellation, timed waits and
receive-pair selection complete the committed batch. Named workers accept owned
arguments and map callbacks share eligible immutable reusable environments
(issue #28). Sending-task join/shutdown guards (issues #12/#74), receiving-worker
join checks (issue #108) and blocking future-result checks (issue #109) landed.

Atomics/mutexes can be shared through scoped loans or `Shared`, with cycle and
lock-held wait checks (issue #326, local PR #366); HTTP client sharing followed
(local PR #369). Parallel terminals drain before return and borrow list sources
in place (issue #455, local PR #571).

See [threads](../design/facilities/03-concurrency/threads.md),
[channels](../design/facilities/03-concurrency/channels.md),
[executors](../design/facilities/03-concurrency/executors.md) and
[parallel operations](../design/facilities/03-concurrency/parallel-map.md).
Broader concurrency/task models stay **B22** in the [backlog](../backlog.md).

## B29: Generic checking

Callable bounds on data, use-site instantiation diagnostics and per-value
argument blame (including aliases/projections) shipped
(issues #40/#518/#519/#526, local PRs #620/#638). Generic class/enum methods are
checked/emitted when used or required by a protocol/destructor
(issue #515, local PR #616); `--check-all-methods` checks the program's own
instances, not dependency/std types (issue #517, local PR #621).
See [generic data](../design/language/07-generics/data.md).
The [library-owned operations investigation](../proposals/completed/library-owned-operations.md)
completed in issue #145 (local PR #177): existing owning wrappers and optional
capabilities meet the assessed use cases; no language or stdlib change was approved.
Future default/final methods and private-hook grants belong to the separate
[protocol-body proposal](../proposals/protocol-method-bodies.md).
Additional bounds/default-body contracts remain **B29** in the [backlog](../backlog.md).

## B32: Callback storage

Owned concrete reusable/consuming environments and worker eligibility work in
ordinary aggregates. Explicit fallible `BoxedCallable` supports heterogeneous
owned shared-call storage (issue #25). See the
[storage decision](../proposals/completed/stored-closures.md) and
[boxed callable reference](../design/language/06-functions/05-boxed-callables.md).
Additional erased modes stay **B32** in the [backlog](../backlog.md).

## B36: Named arguments and defaults

Named functions/methods/constructors have static name binding, source-order
argument evaluation, fresh per-call defaults, callee-local propagation and
nonescaping default loans. See [call rules](../design/language/06-functions/01-signatures-and-calls.md)
and the [completed design](../proposals/completed/named-arguments-and-fresh-defaults.md).
Additional call forms stay **B36** in the [backlog](../backlog.md).

## B37: Ordering

Natural list sorting, swap and multi-target assignment (issue #31), lexicographic
strings (issue #22), allocation-free unstable `sort_by_key`/fallible `sorted`
(issue #64) and tuple relations/heap ordering (issue #111) shipped. Runtime
introsort with heapsort fallback uses generated native key comparators and
cached tuple field offsets (issue #331).
See [lists](../design/facilities/01-builtins/lists.md).
Additional ordering contracts stay **B37** in the [backlog](../backlog.md).

## B72: State and ownership

HTTP assembly no longer needs placeholder heads, empty-string copies or dummy
Results (issues #134–#136). Owned method receivers reuse by-value call rules;
`std.text.Builder.finish()` adopts them. Field moves from owned locals/`self`
are path-tracked with reassignment and cleanup rules (issue #534).
The [owned-response experiment](../proposals/completed/typestate-response-experiment.md)
validated retry ownership, partial-write reporting and exactly-once cleanup,
without implementing real HTTP integration. See [field moves](../design/language/05-ownership/01-bindings-moves-and-copies.md#moving-fields).
Remaining experiments/ownership extensions stay **B72** in the [backlog](../backlog.md).

## B75: Fluent pipelines

All four [proposal stages](../proposals/completed/fluent-iterators.md) shipped: local
borrowed/owned fused plans, adapters/pair callbacks and fallible terminals;
explicit owned `.generator()` conversion; chain/flat_map and callback-free
filtering/reductions; resumable/lendable plans; and explicit chunked parallel
terminals. `std.iter` was removed in favor of this surface (local PR #545).
Generator methods resolve concrete producers (issue #401), known lengths reserve
capacity, and affine cleanup/callback performance fixes landed
(issues #398/#399/#452/#455).

Delivery evidence includes local PRs #540/#543/#546–#548/#551/#552/#554–#556/#560/#562;
issue #371 is fixed. See [pipelines](../design/language/09-iteration/06-pipelines.md)
and [callback measurements](../development/profiling-pipeline-results.md#serial-pipeline-callbacks).
Only the remaining extensions are **B75** in the [backlog](../backlog.md).

## B84: Copy classes

Nongeneric `@copy` classes copy on ordinary use when every field copies and no
`__del__` is declared (issue #535, local PR #655). This also introduced the
closed declaration-annotation mechanism. See [copy classes](../design/language/03-types/classes.md#copy-classes).
Generic `@copy` classes copy when their type arguments copy, and a field that moves for every argument is rejected at the class; the built-in `CopyType` bound requires a type argument that copies (local PR #785). Dictionary keys remain **B74** in the [backlog](../backlog.md).

## B90: Owner-to-view conversion

`&xs`, `&mut xs`, `&s`, reference bindings and `b"..."`/`str` literals convert
to whole views at view types, and `&Shared[T]` converts to `&T` like `Box`
(local PR #784). Byte-reading std APIs, including the prelude byte writers and
every public read-only `&Bytes` input, take `&[u8]`, and `ByteSource` is
removed; `&b"..."[a:b]` views literal data and a shared view argument can view
a call result (local PR #800). See [slice views](../design/language/04-expressions/slices.md#slice-views)
and the [proposal](../proposals/consistency-rules.md#rule-3-an-owner-converts-to-its-view).

## Retired foundations

These original B IDs have no separate active initial-delivery task. A delivery
may have later evolved; use linked references for today's behavior, particularly
dictionary writes and exact target support.

| ID | Delivered scope / decision |
| --- | --- |
| B01 | Linker selection via CLI/Rust options, cc-compatible drivers, verbose output, argument fidelity and deterministic native/mock tests; mold/lld preference (issue #314). [Linking](../design/toolchain/01-building/linking.md). |
| B02 | Application object output and embedded-runtime extraction without a linker on PATH, with external-driver tests. [Commands](../design/toolchain/01-building/commands.md). |
| B03 | Validation of the packaged x86-64 Linux GNU target, runtime triple and pointer layout before emission. This does not claim additional targets shipped. [Runtime packaging](../design/toolchain/02-architecture/runtime.md). |
| B04 | Trusted `.plentyi` C scalar/opaque-pointer imports and native ABI fixtures. [Declarations](../design/facilities/04-interop/trusted-declarations.md). |
| B05 | Ownership-aware pointer/UTF-8/C-string adapters, opaque handles, cleanup and partial/failing transfer fixtures. [Adapters](../design/facilities/04-interop/borrowing-text.md). |
| B06 | Static/shared C exports, scalar/Result adapters, factories/destruction, transfer contracts, embedded interfaces, SHA-256 guards and verification. [Exports](../design/facilities/04-interop/exports.md). |
| B07 | Typed resident runtime loaders, exact metadata/symbol checks, ownership-preserving adapters and allocation-free error tests. [Loading](../design/facilities/04-interop/loading.md). |
| B08 | Function values, explicit reusable/consuming closure environments, factories/inference, callable constraints, captured generator frames and lifecycle cleanup. [Closures](../design/language/06-functions/03-closures.md). |
| B09 | Initial generic classes/enums, `IntType` bounds, inference, generic methods, structural contracts, specialization limits/reuse and allocation/drop checks. [Generics](../design/language/07-generics/index.md). |
| B10 | Box/collection-bounded self/mutual recursive source types and aliases, finite type graphs, and allocation-free cleanup of 100,000-node chains on a 256 KiB stack. [Recursive data](../design/language/03-types/recursive-data.md). |
| B15 | Qualified/nested/or/scalar patterns, inferred generic arguments, whole-scrutinee `case _ as name` and match-valued bindings/assignments/returns (issues #36/#114/#118). Omitting user enum qualifiers was declined. [Matching](../design/language/04-expressions/matching.md). |
| B25 | Concrete inline generator frames, producer specialization and no-allocation construction/move/resume/drop tests. [Generators](../design/language/09-iteration/03-generators.md). |
| B26 | Copyable inline ranges with allocation-free calls, indexing, membership and repeat traversal. [Ranges](../design/facilities/01-builtins/ranges.md). |
| B30 | Ordinary nested updates without enclosing copies/allocations, with saved-index/RHS ordering and alias/failure cleanup tests. Dictionary updates subsequently moved to `insert`/`get_mut` (issue #580). [Assignment](../design/language/04-expressions/assignment.md). |
| B31 | Callable-simplicity audit retained `Callable`/`Closure`, ordinary generic callback records, mismatch guidance and closure source locations (issue #24). |
| B34 | Bounded post-concurrency cleanup/review through local main `736fe80`: ownership, diagnostics, inline storage, cleanup, typed reporting, collection conveniences, compiler scaling, reproducibility and docs. Later channel checks are recorded under B22. |
| B35 | Inline classes/tuples/enums with allocation-free storage construction, recursive heap boundaries and boxed C handles (issue #17). Fallible argument expressions/custom initializers retain their failures. [Classes](../design/language/03-types/classes.md). |

Other retired premises: deterministic runtime cleanup replaces leaked string/heap
storage; strings already carry lengths and embedded NULs; the precompiled runtime
is embedded, not rebuilt from C per program. Function type inference, unit type
arguments in function specializations (issue #110), catch-all matching and the
mdBook migration are delivered. Integer bitwise operations, checked shift counts
and explicit wrapping arithmetic shipped for issue #33.

[Outstanding work](../backlog.md) is not duplicated here.

## Constructor function values

B93's constructor subset (issue #618): class constructors and payload-bearing
variants, including `Some`, `Ok`, and `Err`, are positional function values.
Concrete expected callable results specialize generic constructors; explicit
instances work without context. Pipelines accept constructor values, and shared
or mutable callback arguments can borrow their code addresses. Fallible class
initializers keep `Result[Class, AllocError]` as their callable result. See
[function values](../design/language/06-functions/02-function-values.md) and
[remaining B93 scope](../backlog.md).

## B95: Integer domain types

`Positive[T]` and `NonNegative[T]` for signed integers now provide literal
conversion, fallible run-time conversion, private construction, `.get()`,
domain-preserving arithmetic with checked overflow, ordinary comparisons,
copying, scalar formatting and hashing. Explicit `NonZero.from(positive)` and
`NonNegative.from(positive)` widen without failure or allocation.
Invalid element diagnostics suggest `NonZero[uN]` or `uN` only for unsigned
integers, not floats or other unsupported types (issue #700).

See [integer domains](../design/language/03-types/numbers.md#positive-and-non-negative-integers).
Plain integer dividends accept `NonZero[T]` and `Positive[T]` divisors for
`//`, `%` and augmented assignment, preserving floor semantics while omitting
the zero guard (issue #664). A `NonZero` divisor retains signed division
overflow handling; a `Positive` divisor also removes the impossible `-1`
overflow/remainder guard. Checked blocks infer only the remaining failures.
The std migration is [B95: Std limit domains](#b95-std-limit-domains).

## B95: Positive concurrency counts

Builtin `channel` capacity, `ThreadPoolExecutor` worker/queue counts and
parallel pipeline `chunks` now take `Positive[i64]` (issue #665). Literals
are checked at compile time; computed counts use `Positive.try_from` once.
Channel construction returns `AllocError`; `PoolError` retains only
`Allocation(AllocError)` and `Thread(ThreadError)`. Parallel chunks need no
run-time positivity trap. Existing queue ownership, startup rollback, bounds
and allocation-failure contracts are preserved.

See [channels](../design/facilities/03-concurrency/channels.md),
[executors](../design/facilities/03-concurrency/executors.md) and
[parallel chunks](../design/language/09-iteration/06-pipelines.md).
The remaining std APIs are [B95: Std limit domains](#b95-std-limit-domains).

## B95: Std limit domains

Std parameters and `Limits` fields with a fixed lower bound of 0 or 1 take
`NonNegative[i64]` or `Positive[i64]`, completing migration groups A–C of the
[domain types proposal](../proposals/completed/domain-types.md). This covers
url, kv, markdown, json, toml, xml, regex, template, text, fmt, report, log,
csv, io, process, fs, tempfile, system, archive, compress, image, stat, math,
sketch, http (client, body, form, multipart, typed, websocket), jwt, crypto
and the actor, actor networking, TLS and HTTP server entry points.

The now-impossible `Err` cases are gone: `UrlError.InvalidLimit`,
`BodyError.InvalidLimit`, `KvError.InvalidLimit`, xml `Fault.InvalidLimits`,
`CsvError.OutOfRange`, `BytesError.OutOfRange`, `StartError.Limit` and
`SystemError.InvalidInput` among them. Fixed ceilings (json and xml depth 1024,
regex nesting 1000, template and fmt depth 64, hkdf size 8160, actor heap
limit, image dimensions) trap with a message naming the operation and
argument. `std.actor.net.connection` returns `AllocError`. The
[conventions](../design/stdlib/conventions.md#invalid-arguments) state the rule
for new APIs.

`std.http.Limits.max_line`/`max_head` (`>= 2`) stay `i64` until `Bounded`
(B96). Fixed std ceilings now reject statically known out-of-range arguments
(issue #686), including module-domain constants, named arguments, generated
config constructors, direct field updates and `replace` of a field. Std-only
metadata preserves the existing integer types and runtime traps for dynamic
values; indirect calls and writes through separately borrowed fields keep those
runtime checks. See [the checking mechanism](../design/toolchain/02-architecture/frontend.md#std-ceiling-checking).
Std first used private getter functions for constant limits;
domain-typed module constants (issue #685) replaced them.

## Local dictionary views

B91's dictionary-view consistency slice (issue #624) makes `values()` and
`items()` reusable shared local views with constant-time `len` and pipeline
sources. Set observations, algebra, and filter mutations accept dictionary
key views. Views keep their dictionary borrowed and remain local-only; mutable
value/item iteration stays immediate. See [dictionary views](../design/facilities/01-builtins/dictionaries.md#snapshots-and-views)
and the [remaining B91 scope](../backlog.md).

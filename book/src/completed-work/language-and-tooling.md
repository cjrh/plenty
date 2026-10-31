# Completed language and tooling work

[Completed-work index](../completed-work.md) · [Remaining work](../backlog.md)

These records describe delivered subsets and concluded decisions. References
own current contracts; extensions of a mixed B item stay in the backlog.

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
(issue #599).

See [borrowing](../design/language/05-ownership/02-borrowing.md),
[returned references](../design/language/05-ownership/04-returned-references.md),
[optional references](../design/language/05-ownership/05-optional-references.md)
and [slices/views](../design/language/04-expressions/slices.md).
Outstanding relationships remain **B11** in the [backlog](../backlog.md).

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
(issue #557). Class match patterns shipped with issue #558.
Outstanding conveniences stay **B14/B71** in the [backlog](../backlog.md).

## B16: Values and syntax

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
Remaining inference/value candidates stay **B16** in the [backlog](../backlog.md).

## B17: Tooling and packaging

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
The [owned-response experiment](../proposals/typestate-response-experiment.md)
validated retry ownership, partial-write reporting and exactly-once cleanup,
without implementing real HTTP integration. See [field moves](../design/language/05-ownership/01-bindings-moves-and-copies.md#moving-fields).
Remaining experiments/ownership extensions stay **B72** in the [backlog](../backlog.md).

## B75: Fluent pipelines

All four [proposal stages](../proposals/fluent-iterators.md) shipped: local
borrowed/owned fused plans, adapters/pair callbacks and fallible terminals;
explicit owned `.generator()` conversion; chain/flat_map and callback-free
filtering/reductions; resumable/lendable plans; and explicit chunked parallel
terminals. `std.iter` was removed in favor of this surface (local PR #545).
Generator methods resolve concrete producers (issue #401), known lengths reserve
capacity, and affine cleanup/callback performance fixes landed
(issues #398/#399/#452/#455).

Delivery evidence includes local PRs #540/#543/#546–#548/#551/#552/#554–#556/#560/#562;
issue #371 is fixed. See [pipelines](../design/language/09-iteration/06-pipelines.md)
and [callback measurements](../development/profiling.md#serial-pipeline-callbacks).
Only the remaining extensions are **B75** in the [backlog](../backlog.md).

## B84: Copy classes

Nongeneric `@copy` classes copy on ordinary use when every field copies and no
`__del__` is declared (issue #535, local PR #655). This also introduced the
closed declaration-annotation mechanism. See [copy classes](../design/language/03-types/classes.md#copy-classes).
Generic copy classes remain **B84**; dictionary keys remain **B74** in the [backlog](../backlog.md).

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

# Completed work

Delivery history and retired backlog entries live here, separate from the
[backlog](backlog.md), which is the single maintained list of future work.

- [Language and tooling](completed-work/language-and-tooling.md): language
  foundations, ownership, calls, value parameters and bounded integers, project
  tooling and concurrency.
- [Checked-default validation](completed-work/checked-default-validation.md):
  branch-scoped semantic/native qualification, controlled measurements and limits.
- [Libraries and applications](completed-work/libraries.md): shipped library
  slices, HTTP acceptance, dataframes, plotting and actors.
- [Compiler performance and SIMD](completed-work/performance.md): delivered
  optimizations, measurements and investigations that were closed or deferred.

B IDs are stable. A record here means the **described subset** shipped or its
investigation concluded; it does not mean every conceivable extension of that
B item is implemented. Mixed items keep their remaining scope only in the
[backlog](backlog.md), linked from these records. A completed investigation may
have rejected an implementation rather than shipped one.

These are concise delivery records, not a second reference or a future-work
queue. The [reference](design/index.md) owns current behavior and limits;
[implementation status](design/toolchain/02-architecture/support.md) summarizes
support. Local issue/PR numbers identify records in the shared `issues.db`, not
GitHub records. Reproductions, claims and review state remain there.

## Reconciliation history

The backlog's full reconciliation on 2026-10-22 covered local `main` and merged
local PRs through #477, reference/tutorial content and the shared issues register.
Compiler/columnar follow-ups were checked on 2026-10-24 through #531. Later
merges added the delivered subsets recorded on these pages. The split from the
active backlog was made on 2026-10-31; it does not independently requalify every
historical claim or declare all remaining extensions complete.

B34's bounded cleanup/review, B71's typed HTTP acceptance, B76's initial numeric,
binary and nested dataframe expansion, B17 package tooling and the initial
B39–B70 library slices have landed. Their acceptance checks remain ongoing
regression requirements, not unfinished milestones.

## Maintaining the split

When work ships, update its reference, implementation status and runnable
lessons. Add a short delivery record here with issue/PR evidence and a reference
link. Remove the delivered scope from the active backlog; if an item still has
extensions, retain its ID there and link both ways. Do not copy outstanding
task lists into these pages or recycle retired B IDs.

## Explicit template interpolation contexts

Issue #834 adds [per-placeholder HTML escape contexts](design/stdlib/template.md#escaping-and-inheritance)
for raw template values. Text and quoted-attribute selectors defer encoding to
final emission, preserve bounded rendering, and keep explicit `safe` assertions
separate. Filter chaining, the conservative default and plain-text mode are
documented alongside the [library delivery record](completed-work/libraries.md#b38-library-breadth).

## Typed time and cancellable waits

Delivered B42.

Issue #635 aligns database busy timeouts, JWT validation, cookie lifetimes,
HTTP dates and SSE retry hints with `Duration` and `Timestamp`. WebSocket, SSE,
readiness and child deadline waits accept caller cancellation while preserving
server shutdown behavior. [`std.sync`](design/stdlib/sync.md#typed-deadline-waits)
provides typed channel, future, token and two-channel selection waits; explicit
millisecond primitives remain as low-level compatibility APIs (B42).

## Registered template fragments

Issue #835 adds [template includes](design/stdlib/template.md#reusable-fragments)
with explicit object contexts, isolated loop and inheritance scopes, and shared
render budgets. Reusable headers and row forms render directly without
application string concatenation or raw-markup trust assertions. The
[library record](completed-work/libraries.md#b38-library-breadth) also records
the delivered template subset.

## CPU availability and system inspection assessment

Delivered part of B38 in issue #716.

[`std.system.available_parallelism`](design/stdlib/system.md#available-parallelism)
provides a positive, fallible worker-count estimate, with bounded Linux affinity
and visible cgroup quota handling plus native Apple/Windows backends. Pool
examples teach explicit caller fallback. The
[coverage/backend assessment](design/stdlib/system-backends.md) records the
psutil-style platform gaps and Rust backend tradeoffs; the
[library delivery record](completed-work/libraries.md#b38-library-breadth) links
to the remaining B38 scope.

## Template literal whitespace control

Issue #837 delivers opt-in delimiter markers for adjacent ASCII source
whitespace, preserving default output, compact negative literals and original
byte diagnostics. See [the reference](design/stdlib/template.md#whitespace-control)
and [the library delivery record](completed-work/libraries.md#template-literal-whitespace-control).

## Bounded template conditions

Issue #836 adds [scalar comparisons, boolean expressions and `elif`](design/stdlib/template.md#expressions-and-conditional-branches)
so navigation and selection markup can use raw JSON IDs. Registered postorder
expression trees have an aggregate node cap and bounded depth; runtime boolean
operators short-circuit under the shared work budget. Strict scalar equality,
filter precedence and atomic registration remain explicit. Filter arguments,
arithmetic, ordering comparisons and calls remain unsupported.

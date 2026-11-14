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
  optimizations, measurements and investigations that were closed or deferred,
  including the [k-nucleotide counting-loop investigation](completed-work/performance.md#k-nucleotide-counting-loop-investigation-issue-743)
  (issue #743).
  [Byte method access](completed-work/performance.md#byte-method-access-issue-741)
  records the narrow checked helpers delivered for issue #741 and the
  [loop-cached byte access](completed-work/performance.md#loop-cached-byte-access-issue-843)
  of issue #843, and the
  [one-word reference reads](completed-work/performance.md#one-word-reference-reads-issue-844)
  of issue #844.

[Selected-field column zip](completed-work/performance.md#selected-field-column-zip-issue-888)
records the shared-cursor lowering and retained instruction evidence for issue #888.
[Conditional callback captures](completed-work/performance.md#conditional-callback-captures-issue-891)
records the removal of the remaining immutable fold-capture reloads.

The [Rust-source backend decision](completed-work/language-and-tooling.md#rust-source-backend-proposal-rejected)
records the proposal's rejection on 2026-11-14; its experiments remain historical.

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

## Derived serialization

Issue #781 completes B77 with shared JSON/TOML codecs for `@serial` classes and
enums, validated construction and defaults, canonical bytes, bounded reader
loading and measured costs. See the
[delivery record](completed-work/language-and-tooling.md#b77-derived-serialization-issue-781).

## Native qualification build reuse

Issue #850 adds isolated dependency caches and concurrent compiler/dataframe
builds to [native OS qualification](development/ci-workflow.md#native-os-qualification),
while preserving packaging audits and smoke coverage. See the
[delivery record](completed-work/language-and-tooling.md#native-qualification-build-reuse-issue-850).

## Declared template context shapes

Issue #838 adds [schema preflight and checked request rendering](design/stdlib/template-schema.md)
for every reachable field path, union alternative, loop and effective composed
layout. Runtime JSON rendering stays available; validation makes no compile-time
claim. See the [library delivery record](completed-work/libraries.md#declared-template-context-shapes).

## Text ownership comparison

The [Text operation tables](design/facilities/01-builtins/text.md#passing-assigning-and-copying-text)
spell out accepted and rejected argument, binding, slice, and explicit-copy
forms, including borrow lifetimes, backing storage, and nested results.
The function-argument table uses multiline signature/call excerpts and explains
when borrowing avoids reference-count work. It also shows whole-string and
substring arguments to `&substr`, including implicit borrowing of owned strings
at calls (issue #847), and explains the view interface's current operation limits.
The binding table keeps expressions on one line, wrapping their explanations
within the ordinary page width.

The reference now compares [Plenty text with Python, Rust, and C](design/facilities/01-builtins/text-compared.md),
including assignment costs, immutable storage, mutable construction, borrowed
views, and Unicode indexing. The comparison tables use a wider screen layout
and independently scroll on narrow screens. This documents existing behavior.

## Pattern matching comparison

The [Python and Rust comparison](design/language/04-expressions/matching.md#compared-to-python-and-rust)
clarifies arm-local shadowing versus Python captures, unsupported Plenty guards,
and Rust similarities and value-position differences. It documents existing
behavior with an executable shadowing example; no language feature is added.

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
millisecond primitives remain as low-level compatibility APIs (B42). Issue #792
makes the reference, tutorial and skill lead with the cancellable waits and
state that the channel, selection and future millisecond waits take no token,
with a lesson that ends a long selection on `SIGTERM`.

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
filter precedence and atomic registration remain explicit. A compatibility
follow-up preserves all 16,384 legacy negated placeholders under default limits
with a 32,768-node budget. Filter arguments,
arithmetic, ordering comparisons and calls remain unsupported.

## Checked HTML template rendering

Issue #839 adds opt-in structural preflight and inferred text/quoted-attribute
escaping for a bounded HTML subset. See [the contract](design/stdlib/template-html.md)
and [the delivery record](completed-work/libraries.md#checked-html-template-rendering).

## Bounded JSON pretty output

Issue #782 adds shared bounded pretty output for JSON events, trees and typed
values. See the [delivery record](completed-work/libraries.md#bounded-json-pretty-output-issue-782).

## HTTP response dates

Issue #786 adds automatic response dates with caller overrides and shared
bounded formatting. See the [delivery record](completed-work/libraries.md#http-response-dates-issue-786).

## Serialization diagnostics

Issue #783 adds bounded readable JSON/TOML causes, typed field paths and opt-in
source excerpts. See the [delivery record](completed-work/libraries.md#serialization-diagnostics-issue-783).

## HTTP malformed-capture responses

Issue #787 adds bounded custom pages for rejected captures while keeping
malformed input out of typed handlers. See the
[delivery record](completed-work/libraries.md#http-malformed-capture-responses-issue-787).

## Pool scheduler guidance (issue #895)

The [helper-loop lesson](tutorial/11-concurrency/scheduler-loops.md) and
[partial-preparation example](tutorial/11-concurrency/scheduler-preparation.md)
teach accepted helper-based submission/reception, receiver-last exit cleanup,
cooperative cancellation and typed-error preservation. The agent skill links
the same guidance. [Executor explanations](design/facilities/03-concurrency/executors.md#helper-based-schedulers)
distinguish scope-exit cleanup from mid-call shutdown and direct field borrowing
from the aggregate-argument gap. This changes guidance, not compiler acceptance.
The guidance distinguishes the bounded capture-free `Callable` exception
(issue #893) and discovered-versus-assumed shutdown provenance (issue #894),
delivered separately under [native concurrency](completed-work/language-and-tooling.md#b22-native-concurrency),
from remaining conservative helper limitations. Capability-bearing and erased
unknown calls, missing bodies and unrelated owned pools remain conservative;
neither the exception nor diagnostic witnesses establish deadlock freedom.
Tutorial integration (including skill fixtures), source links, Python helper
tests and the complete versioned-site build pass. Remaining concurrency scope stays in
[B22](backlog.md#scope-of-follow-on-work).

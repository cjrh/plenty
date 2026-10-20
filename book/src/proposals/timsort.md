# Timsort for stable, adaptive sorting

**Status: Not implemented.** This proposes an algorithm and an API direction,
not an approved migration. [List sorting](../design/facilities/01-builtins/lists.md)
and the [key-sorting tutorial](../tutorial/04-collections/key-sorting.md) define
today's contracts. No implementation or performance improvement ships with this
proposal. Scheduling belongs to [B37](../backlog.md); issue #331 records the
existing sorting performance problem.

## Why consider it

Stable sorting preserves the input order of equal keys. Records already ordered
by time can then be sorted by category without losing their time order within
each category. This should be easy to request without adding an original-position
tie breaker or constructing a separate decorated list. Exploiting existing order
also suits logs, grouped records, and incrementally updated data.

Today `sort()` is infallible and allocation-free. The runtime uses Rust's
unstable slice sort for two-word rows and three-way introsort (with a heapsort
fallback) for wider inline rows
(`plenty-runtime/src/entries.rs`). `sort_by_key` uses the same introsort for every
row width, calling a generated comparator (`src/codegen/sorting.rs`) that
recomputes both keys per comparison. `sorted`
explicitly copies, then invokes the same natural-order sort. Keys cannot return
references or `Result`, and user-defined natural order is still a
[separate proposal](user-type-ordering.md).

Issue #331 identified runtime element access, row movement, and descriptor
comparison costs; the runtime-driven key sort and per-sort tuple and relocation
plans removed most of them. Changing algorithms alone does not remove such costs. Timsort
is a candidate for stable sorting and locality improvements, not evidence that
it will beat the current scalar path or fix that issue by itself.

## Algorithm recommendation

Prototype Timsort: detect existing ordered runs, extend short runs with stable
binary insertion, and merge adjacent runs using bounded temporary storage.
Galloping searches can reduce comparisons when one run repeatedly wins.
CPython's [versioned design notes](https://raw.githubusercontent.com/python/cpython/v3.14.0/Objects/listsort.txt)
describe these mechanisms and their memory tradeoff.

Specify the merge scheduler separately. Classic Timsort uses run-length stack
invariants; modern CPython uses Powersort scheduling within its adaptive sort.
Compare a corrected classic policy with Powersort before selecting Plenty's
implementation. [Munro and Wild's paper](https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ESA.2018.63)
gives Powersort a nearly optimal merge-cost bound relative to the existing runs.
That bound concerns merge work, not guaranteed wall-clock optimality. If selected,
describe Plenty's implementation as Timsort-style run handling with Powersort
scheduling, rather than claiming it uses the classic Timsort policy.

The required contract is stable ordering, O(n log n) worst-case comparisons,
O(n) comparisons for a single ordered run, and documented workspace bounds.
Constants and insertion/galloping thresholds are implementation details. A
simple first version should reverse only *strictly* descending runs: reversing
equal elements would destroy stability. For descending output, reverse the
comparison direction, not the final ascending array. Forward merges take from
the left on equality; backward merges take from the right.

## Surface and compatibility

The recommended eventual teaching surface is `items.sort()?` and
`items.sort_by_key(key)?`: stable order with explicit allocation failure. Keep
infallible, allocation-free operations under explicit `sort_unstable` and
`sort_unstable_by_key` names. These are proposed spellings. Stability is a useful
default, but it does not make an allocating algorithm optimal for every purpose.

Changing existing return types is a source compatibility change. Algorithm
adoption does not authorize it. A compatible first release could introduce
`sort_stable` and `sort_stable_by_key`, each returning `Result[(), AllocError]`,
while retaining the existing methods unchanged. A later naming decision must
include migration diagnostics, reference/tutorial updates, and call-site changes.
`sorted` already returns `AllocError`, so a stable implementation can preserve
its signature and source-preservation guarantee, while documenting extra memory.

Other choices have different costs:

| Choice | Consequence |
| --- | --- |
| Keep stable methods permanently explicit | Preserves compatibility and makes scratch allocation visible; ordinary key sorting still loses ties unless the caller chooses stability. |
| Try scratch, fall back to allocation-free unstable sorting on allocation failure | Can preserve an infallible return type, but introduces hidden allocation and cannot promise stability. This does not meet the proposed stable contract. |
| Fall back to a stable in-place merge | Can preserve stability and avoid returning allocation failure, but simple rotation-based merges can raise worst-case work to O(n log² n); sophisticated bounded-space algorithms add substantial complexity. Attempting allocation still changes today's allocation-free promise. |
| Caller-owned reusable workspace | Makes repeated allocation avoidable; useful later, but capacity/type/lifetime management should not be required for the ordinary call. |

Prefer a fallible stable operation over either fallback for the first
implementation. Preserve an explicit allocation-free route, and decide the
default names separately after measurements and compatibility review.

## Storage, failure, and callbacks

For a straightforward row-merging implementation, reserve at most `floor(n/2)`
whole rows before any mutation. Count bytes using the runtime row stride,
including inline payloads, not Python's pointer-sized estimates. Check all size
arithmetic. Use a run stack whose capacity is justified for the chosen policy
and supported length range. Allocation or size failure returns `AllocError`
with the original list unchanged. Empty/singleton lists require no workspace;
a read-only single-run check can avoid reserving for already ordered input.
Pre-reservation deliberately trades potentially unused memory for a simple
failure guarantee; adaptive buffer growth would need a weaker guarantee or
rollback and is not the initial recommendation.

Moving a row transfers ownership; it must not clone, drop, or retain each
element on every merge step. Relocate inline addresses before any comparison
observes the row, including rows held in scratch storage. Every original value
must have exactly one live owner and remain available for destruction exactly
once. Existing swap-based safety does not automatically establish merge safety.
A narrow audited runtime operation is preferable to exposing unchecked moves
to Plenty programs. Keep safe orchestration in Plenty where it is efficient;
measure the boundary costs identified in issue #331.

Initially retain current key rules: shared `&T` input, reusable shared-call
callback, direct orderable result, consistent keys, and no concurrent access to
the exclusively borrowed list. Recompute keys; do not promise a call count or
evaluation order. Preserve signed-zero, NaN, string, and tuple total ordering.
Key callbacks remain responsible for explicit allocations inside themselves.
Caching keys once is a separate API decision with additional owned storage and
observable callback semantics; fallible keys need their own cleanup/error design.

Inconsistent keys must never cause memory unsafety, ownership loss, or an
unbounded loop; sortedness is only guaranteed for a consistent order. Internal
index/stack checks must not rely on comparator consistency. Plenty fatal traps
do not unwind, so this proposal promises neither rollback nor recovery after a
callback trap. Ordinary allocation failure occurs before rearrangement.

## Evidence required for adoption

The [verification work by de Gouw and de Boer](https://ir.cwi.nl/pub/23712)
found errors in widely used Timsort implementations. Review stack invariants,
capacity proofs, overflow checks, and merge termination explicitly; copying a
familiar implementation is insufficient. Test adversarial run lengths as well
as random permutations. Include equal-key identity tags in both directions,
insertion and galloping boundaries, empty/singleton inputs, NaNs/signed zeros,
wide inline records, affine owners/destructor counts, and injected allocation
failures that verify unchanged input. Check callback loan rejection and
inconsistent-callback memory safety. Differential tests must compare both
sortedness and stable identity order, not just values.

Benchmark scalar, string, tuple, and keyed-record inputs: random, sorted,
reverse, duplicate-heavy, almost sorted, and uneven runs. Record comparison and
key-call counts, moved bytes, peak scratch, allocations, and elapsed time; retain
the current sort as the baseline. Recreate issue #331's workloads and use the
collection benchmarks without treating old measurements as new evidence.
Compare classic Timsort, Powersort scheduling, a simple stable mergesort, and
the existing allocation-free implementation. A different unstable runtime sort
could independently address #331; record-layout work in issue #330 is separate.
Publish regressions as well as wins before choosing defaults. Any implementation
PR must update contracts and executable lessons together; this proposal alone
changes neither.

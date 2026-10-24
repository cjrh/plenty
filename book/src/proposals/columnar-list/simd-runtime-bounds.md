# SIMD counts with runtime bounds

The next bounded step after [constant comparisons](simd-counts.md) recognizes
immutable runtime scalar captures and two total comparisons joined by `and`.
This makes queries such as “count scores between these two input limits” use the
same four-lane SSE2 path without exposing vectors, alignment, tails or hardware
selection in the source. The [tutorial](../../tutorial/special-purpose/columnar-lists/02-counting.md)
shows the supported idiom; the [reference](../../design/facilities/01-builtins/clists.md)
defines its limits.

## Legality boundary

Predicate recognition now has separate bounded metadata rather than expanding
general leaf-function inlining to control flow. It accepts one comparison or
the exact checked-IR short-circuit form for two comparisons joined by `and`.
Both compare the item with a same-type constant or an immutable scalar capture;
either operand order is accepted. Captures are limited to two by-value `i32` or
`u32` values matching the column. The actual closure environment is read once
before the vector loop, preserving its captured snapshot rather than rereading
the original source binding.

Two total comparisons can be evaluated eagerly as vector masks because neither
can call, allocate, mutate, trap or read externally mutable state. Combining
their masks therefore preserves observable short-circuit behavior. Borrowed
captures, mutable closures, arithmetic, calls, `or`, extra comparisons and extra
pipeline adapters retain the scalar path. The original scalar loop handles the
tail, with the same bounds and 64-bit count guarantees as before.

## Validation

Tests compare normal and optimized scans with scalar expectations for all six
comparisons, inclusive/exclusive range endpoints, reversed/equal bounds, lengths
0–33, and explicit signed/unsigned minimum, maximum and sign-bit boundaries.
They check snapshot preservation after the source binding changes and across
successive calls. Short-circuit side effects and skipped overflowing arithmetic
remain scalar. IR/assembly checks confirm vector emission for runtime, named,
constant/captured mixed and reversed comparisons, and scalar fallback for
borrowed captures, arithmetic and disjunction.

The protected-page fixture now covers constant, runtime-threshold and range
scans over lengths 0–129 and a null empty owner. Each scenario checks actual
compiled Plenty functions against a scalar oracle.

## Performance method

`experiments/simd/clist/run.py --scenario threshold` and `--scenario range`
compare optimized pipelines with explicit scalar column loops. Both receive
their bounds as runtime function arguments from the C fixture. The scalar
reference has no per-row callback call; measurements therefore do not inflate
the SIMD benefit with callback overhead. The original constant-only reports
remain historical evidence, since their captured scalar reference now qualifies
for SIMD.

Each scenario uses one compiled object, four verified code placements, randomized
execution order, one excluded warmup and three samples per variant. Internal
elapsed time covers repeated scans; instruction/cycle counts also include fixture
startup. Small scans expose setup/tail costs, and large hot scans are not a claim
about cold-data bandwidth or application-wide speedup.

## Final measurements

Measured on 2026-10-24 on the Ryzen 7 7840HS, pinned to CPU 8, with the final
Cranelift 0.137.0 release compiler. Two monitoring windows each sampled host
processes 24 times at half-second intervals during the respective runs and saw
no concurrent `cargo` or `rustc`. Raw reports are retained as
`experiments/simd/clist/measurements-runtime-threshold.json` and
`measurements-runtime-range.json`, including compiler/object hashes, checksums,
function addresses and all 144 measured executions across the two scenarios.

Each range spans the four placement medians:

| Query | Rows | Scalar ns/row | SIMD ns/row | Scalar instructions/row | SIMD instructions/row |
| --- | ---: | ---: | ---: | ---: | ---: |
| Runtime threshold | 7 | 1.613–1.980 | 2.152–2.212 | 36.867 | 40.010 |
| Runtime threshold | 8,192 | 0.625–0.931 | 0.107–0.214 | 11.034 | 3.039 |
| Runtime threshold | 262,144 | 0.623–0.943 | 0.108–0.215 | 11.058 | 3.058 |
| Runtime range | 7 | 1.957–2.301 | 2.978–3.055 | 41.581 | 53.438 |
| Runtime range | 8,192 | 0.856–1.318 | 0.162–0.218 | 15.034 | 4.542 |
| Runtime range | 262,144 | 0.845–1.270 | 0.160–0.212 | 15.063 | 4.563 |

At matched placements, the large threshold scans are **2.91–8.71× faster**
and range scans **4.41–8.01× faster** than the explicit scalar loops.
Instruction reductions are stable across placements; elapsed time remains
placement-sensitive, so the whole range matters. Seven-row pipeline scans are
**12–34% slower** for thresholds and **30–56% slower** for ranges than those
loops: closure/vector setup and scalar tails dominate. This does not compare
against the old scalar callback pipeline, and is not evidence that every small
query should vectorize. No new profitability threshold is inferred from these
three sizes alone.

Generated function sizes are 640 bytes for the threshold pipeline versus 309
for its scalar loop, and 752 versus 352 for the range. The extension keeps the
existing SSE2 baseline and does not add target dispatch or parallel execution.

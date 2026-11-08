# Polars embedding evidence

The [proposal](../polars-dataframes.md) describes the boundary and semantic limits.
Reproduction scripts and raw observations are in `experiments/polars-embedding/`.
The experiment is separate from the compiler's Cargo workspace and dependency
graph. Normal Plenty builds do not compile or link Polars.

## What runs

- A Plenty-native facade constructs two typed columns from Plenty lists.
- Explicit expressions filter values, derive a column, group by a key and sum.
- The same query runs on a native lazy Parquet scan, returning values to Plenty.
- Negative cases cover missing columns, mismatched input lengths, bounds and
  wrong-type extraction. Null and NaN remain distinct, negative zero survives
  extraction, empty schemas work, and a query outlives its source builders/frame.
- Private constructors prevent source programs from manufacturing invalid native
  owners. Rust tests exercise null handles, unchanged outputs after errors and
  caught ordinary panics.

## Measurement method

`run.py` compares the same deterministic data and equivalent filter/count or
filter/multiply/group/sum work in Plenty with Polars, direct Rust with Polars,
and a hand-written Plenty `clist` implementation. Input generation, import and
repeated query execution are separate measurements. The grouped `clist` code
specializes to eight known integer groups; it is not a general hash grouping
implementation. All results are checked against an independent checksum.

Polars queries are built once and collected repeatedly, with five warmups. Query
times include collection and copying the scalar or eight aggregated values out.
They do not include input conversion or startup. Trials rotate implementation
order. One-thread and four-thread settings use distinct physical cores; `clist`
remains serial. Tiny and million-row inputs expose dispatch overhead as well as
scan throughput. The startup experiment measures process launch with warm file
cache, not cold disk load or the first query's thread-pool initialization.

The built shared engine is copied beside Plenty binaries and found through
`$ORIGIN`. Compilation timings measure optimized whole-program compile/link
with caches disabled and a repeated cache hit with the engine already built.
They are not a claim of separate Plenty module compilation.

## Observations

Measured on 2026-10-25, AMD Ryzen 7 7840HS, Rust 1.99/LLVM 23.1.1. The engine
uses explicit x86-64-v3 flags, stable Rust and no Polars nightly feature. Plenty
programs retain their normal x86-64 baseline. CPU affinity is 2 for one thread
and 2,4,6,8 for four threads. Seven rotated trials are recorded, with 500 query
repetitions at 1,024 rows and 20 repetitions at one million rows.

| Rows | Query | Polars threads | Plenty + Polars | Direct Rust + Polars | Plenty `clist` |
| --- | --- | --- | --- | --- | --- |
| 1,024 | Filter/count | 1 | 41.3 µs | 39.8 µs | 0.96 µs |
| 1,024 | Filter/group/sum | 1 | 72.9 µs | 70.1 µs | 4.11 µs |
| 1,000,000 | Filter/count | 1 | 0.728 ms | 0.756 ms | 0.935 ms |
| 1,000,000 | Filter/group/sum | 1 | 4.574 ms | 7.982 ms | 3.941 ms |
| 1,000,000 | Filter/count | 4 | 0.710 ms | 0.709 ms | 0.961 ms |
| 1,000,000 | Filter/group/sum | 4 | 2.721 ms | 2.723 ms | 3.922 ms |

The large four-thread grouped query is about 1.44× faster than this specialized
`clist` loop, while small queries pay substantial engine overhead. The count
comparison uses `f64`, not `clist`'s SIMD-specialized integer filter/count path.
The wrapper and direct Rust paths are close in most cases. One-thread large
grouping is an unresolved exception: the separate direct Rust executable is
consistently slower. Removing an unnecessary rename from its plan did not remove
the difference. These are distinct native binaries and construction paths; this
bounded spike does not attribute that difference to wrapper overhead or claim
that calling Polars from Plenty speeds up Polars itself.

For one million rows, constructing the two Plenty input lists costs about
95–96 ms, then copying into Polars costs about 36 ms. Direct Rust constructs its
inputs in about 5.8 ms and imports them in about 6.3 ms. Building `clist` from the
same Plenty lists costs about 41 ms. A single sub-millisecond query does not
amortize these conversions. Keep data inside the engine across operations, read
files directly where appropriate, and evaluate bulk transfer before promising
efficient mixed Plenty/Polars workloads.

The instruction-count cross-check covers the **whole process**, including input
generation/import, 100 grouped queries and Polars' five warmups. It records
about 9.62 billion user instructions for Plenty + Polars and 12.18 billion for
`clist`, yet the corresponding cycles are about 2.78 and 2.46 billion. These are
not isolated kernel counts or a substitute for the measured query times.

## Build and distribution costs

The clean optimized engine build takes **279.9 seconds** at three Cargo jobs,
with dependencies already downloaded. GNU time reports 1,709,584 KiB maximum
process RSS; this is not total parallel-build memory. The lockfile contains 332
packages including the adapter. A later direct-baseline-only rebuild takes
1.12 seconds. The clean-build observation predates removal of an unused helper
and that baseline adjustment; it measures the same dependency and engine scope.

The shared library is **90.4 MiB**, stripped of debug information. A Plenty
benchmark executable is about 3.2 MiB and uses that shared artifact. The
optimized Plenty/Polars benchmark compiles and links in about **53 ms without
caches**, or **41 ms with a warm cache**; `clist` is about 49/41 ms. These
process-level times include compiler startup and runtime extraction. They do
not include building the engine. This is a good separation of engine build cost
from everyday Plenty compilation, but a material distribution dependency.

Warm-file-cache launch with a single expression construction is about **2.79 ms**,
versus **1.15 ms** for the plain executable. That includes subprocess and
`taskset` overhead; it does not measure a cold launch or first-query latency.

## Assessment

Embedding is tractable with the existing language boundary. Explicit expressions
reach Polars' optimizer, broad analytical operations and Parquet work without a
compiler change, and the engine can be shipped precompiled. The results support
prioritizing this direction over building those analytical facilities ourselves.

They do not establish a drop-in replacement for `clist`. Scalar/list transfer,
typed versus discovered schemas, null/NaN/overflow semantics, diagnostics and
native allocation failure need deliberate contracts. Tiny collection operations
remain much cheaper without a query engine. Current `clist` users need not
migrate as a consequence of this experiment.

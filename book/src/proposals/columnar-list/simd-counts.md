# Automatic SIMD filter/count

The first automatic SIMD implementation recognizes one `i32` or `u32` column,
one filter comparing its borrowed item with a same-type constant, and `count()`.
It runs in optimized (`--release`) native builds. The
[reference](../../design/facilities/01-builtins/clists.md#automatic-simd-counts)
defines the supported pattern; the [tutorial](../../tutorial/06-data/11-columnar-records.md)
shows ordinary source code. There is no public SIMD API or new syntax.

## Why this boundary

Recognition inspects the checked loop and a bounded scalar leaf predicate. It
accepts exactly one comparison, either in a named function or a closure without
captures. Calls, arithmetic, extra adapters and other shapes keep their existing
scalar path. The original scalar loop is retained for the tail. No callback
effects, traps, floating-point operations or allocation failures are reordered.

The source has a stable borrowed pointer and length. Fresh zero counters and
the known increment establish that the count fits `i64`. Four-lane loads only
run below the length rounded down to a multiple of four; they have no alignment
promise. Comparison masks widen to two 64-bit accumulators before summing, so
large columns cannot overflow 32-bit lane counters. The final scalar item local
is preserved as well as index and count.

Cranelift 0.131.1 emits 128-bit SSE2 instructions for this pattern. The current
machine (AMD Ryzen 7 7840HS) has AVX2 and several AVX-512 features, but this slice
does not need them. Using the existing target baseline keeps binaries portable
among supported x86-64 machines and avoids runtime dispatch overhead and policy.
This is not a claim that Cranelift can exploit all this CPU's wider vector
features; the earlier [backend evidence](../std-simd/evidence.md) still applies.

## Validation and reproduction

Integration tests compare all six signed/unsigned comparisons over lengths
0–129 with scalar expectations, including extreme bit patterns, empty owners,
column offsets and every four-lane remainder. Separate cases retain effects,
captures, checked arithmetic and additional adapters on the scalar path. Dump
tests check actual vector IR and packed comparison instructions.

The experiment in `experiments/simd/clist/` compiles the actual Plenty pipelines,
then links a C fixture against their generated functions. It constructs read-only
internal-ABI owners solely to place columns immediately before a protected page.
Lengths 0–129 and a null empty owner pass. Odd lengths need four poison bytes
before the guard to keep the header aligned; scalar-oracle comparisons detect
counting that padding. This fixture is not a supported foreign clist interface.

Run from the repository root:

```sh
cargo build --locked --release --bin plenty
python3 experiments/simd/clist/run.py target/release/plenty --out /tmp/clist-simd-check --verify-only
python3 experiments/simd/clist/run.py target/release/plenty --out /tmp/clist-simd-bench
```

The benchmark uses a captured threshold to retain the scalar pipeline and a
constant threshold to enable SIMD, both comparing against five. It builds one
object and relinks it at four text addresses, 16 bytes apart, then shuffles
scalar/SIMD executions. Each variant has one warmup and three measured samples.
Internal timings exclude input construction; `perf` instructions/cycles include
startup and construction. Tiny calls therefore include substantial fixed scan
setup. This is a hot repeated microbenchmark, not an application or memory-
bandwidth claim. Code placement matters, as [issue #437's research](placement-research.md)
demonstrated; do not select just the best scalar/SIMD address pair.

## Final measurements

Measured on 2026-10-24 on the Ryzen 7 7840HS, pinned to logical CPU 8, with a
fresh release compiler based on `f625d54d`. Host process monitoring sampled
throughout the final run and observed no concurrent `cargo`/`rustc` work.
The retained `experiments/simd/clist/measurements.json` records all 72 measured
executions, checksums, actual function addresses, instruction/cycle counts,
compiler/object hashes and the method. Earlier exploratory runs are not mixed in.

Each cell below spans the four placement medians, not individual samples:

| Rows per call | Scalar ns/row | SIMD ns/row | Scalar instructions/row | SIMD instructions/row |
| ---: | ---: | ---: | ---: | ---: |
| 7 | 2.036–2.332 | 1.735–2.050 | 42.724 | 38.438 |
| 8,192 | 0.625–0.939 | 0.155–0.162 | 13.037 | 3.287 |
| 262,144 | 0.636–0.931 | 0.156–0.159 | 13.058 | 3.308 |

At matched link placements, the two larger sizes improve **3.87–6.05×** in
elapsed time with roughly **four times fewer instructions**. SIMD uses about
0.77–0.80 cycles/row, while the scalar control varies between about 3.0 and 4.6.
The small case improves only 1.02–1.34×; setup costs dominate and the captured
scalar threshold has extra fixed setup. This compares two paths in the same
compiler, not identical source compiled before and after the change.

The SIMD function occupies 608 bytes including its constant pool; the scalar
control occupies 525 bytes. Assembly contains unaligned `movdqu`, `pcmpgtd`,
mask shuffles and `paddq`/`psubq`, followed by the scalar tail. The comparison
does not call a predicate per row. The gains justify this small recognition
slice; they do not establish a general backend ranking, wider-vector benefit,
or speedup for other predicates and workloads.

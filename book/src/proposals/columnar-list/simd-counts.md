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

Cranelift 0.137.0 emits 128-bit SSE2 instructions for this pattern. The current
machine (AMD Ryzen 7 7840HS) has AVX2 and several AVX-512 features, but this slice
does not need them. Using the existing target baseline keeps binaries portable
among supported x86-64 machines and avoids runtime dispatch overhead and policy.
This is not a claim that Cranelift can exploit all this CPU's wider vector
features; fresh [backend upgrade probes](simd-backend-upgrade.md) establish the
current width boundary and distinguish useful new backend features.

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
fresh Cranelift 0.137.0 release compiler incorporating main `539f7ee4`.
Host process monitoring sampled 15 times at half-second intervals
throughout the final run and observed no concurrent `cargo`/`rustc` work.
The retained `experiments/simd/clist/measurements.json` records all 72 measured
executions, checksums, actual function addresses, instruction/cycle counts,
compiler/object hashes and the method. Earlier exploratory runs are not mixed in.

Each cell below spans the four placement medians, not individual samples:

| Rows per call | Scalar ns/row | SIMD ns/row | Scalar instructions/row | SIMD instructions/row |
| ---: | ---: | ---: | ---: | ---: |
| 7 | 2.023–2.382 | 1.652–1.945 | 42.724 | 38.010 |
| 8,192 | 0.635–0.976 | 0.105–0.212 | 13.037 | 3.037 |
| 262,144 | 0.635–0.972 | 0.104–0.212 | 13.058 | 3.058 |

At matched link placements, the two larger sizes improve **3.00–6.16×** in
elapsed time with roughly **4.3 times fewer instructions**. SIMD uses about
0.52–1.04 cycles/row, while the scalar control varies between about 3.0 and 4.8.
The small case improves only 1.05–1.44×; setup costs dominate and the captured
scalar threshold has extra fixed setup. This compares two paths in the same
compiler, not identical source compiled before and after the change.

The previous 0.131.1 run is retained as
`experiments/simd/clist/measurements-cranelift-0.131.json`. The new vector loop
retains its comparison constant in a register and executes about 0.25 fewer
instructions per row on the larger inputs. Placement sensitivity is stronger
in this run, so the upgrade does not imply a uniform elapsed-time improvement.

The SIMD function occupies 608 bytes including its constant pool; the scalar
control occupies 525 bytes. Assembly contains unaligned `movdqu`, `pcmpgtd`,
mask shuffles and `paddq`/`psubq`, followed by the scalar tail. The comparison
does not call a predicate per row. The gains justify this small recognition
slice; they do not establish a general backend ranking, wider-vector benefit,
or speedup for other predicates and workloads.

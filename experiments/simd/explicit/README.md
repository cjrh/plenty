# Explicit portable SIMD probes

Run from the repository root with a compiler built from this worktree:

```sh
PLENTY_CACHE=off target/debug/plenty --release --compile experiments/simd/explicit/rounds.plenty -o /tmp/simd-rounds
/tmp/simd-rounds
PLENTY_CACHE=off target/debug/plenty --release --compile experiments/simd/explicit/float_rounds.plenty -o /tmp/simd-float-rounds
/tmp/simd-float-rounds
PLENTY_CACHE=off target/debug/plenty --release --compile experiments/simd/explicit/throughput.plenty -o /tmp/simd-throughput
/tmp/simd-throughput
```

Each executable prints seven rows: sample, scalar nanoseconds, explicit-vector
nanoseconds, checked result. Each side runs one million iterations. Both sides
use the same scalar operation order within each lane, and verify identical final
results. The scalar baselines keep independent primitive accumulators, with no
artificial callback or collection overhead. Timings run scalar first and vector
second in each sample; treat them as bounded probes, not a general benchmark
suite or a precise hardware attribution.

- `rounds.plenty`: four `u32` modular multiply/add recurrences. Baseline SSE2
  multiplication is a multi-instruction sequence; scalar multiplication can win.
- `float_rounds.plenty`: four `f32` divide/add recurrences. One dependent vector
  chain exposes instruction latency and can be close to scalar speed.
- `throughput.plenty`: sixteen `f32` recurrences, represented by four vectors.
  Independent vectors exercise throughput without changing arithmetic order.

For instruction counters, compile `scalar.plenty`, `vector.plenty`,
`float_scalar.plenty` and `float_vector.plenty` the same way and run, for example,
`perf stat -e instructions:u,cycles:u /tmp/simd-vector`. These wrappers run ten
million iterations and print a checksum. Counters include process startup and
printing; that fixed overhead is small at this size. Do not run the timing probes
alongside builds. Retain unfavorable outcomes and record machine/load context.

Inspect the actual generated loop with:

```sh
target/debug/plenty --release --dump clif-opt,asm --dump-filter vector_rounds experiments/simd/explicit/rounds.plenty -o /tmp/simd-dump
```

The final loop should keep numeric vectors in registers, without the initial
prototype's per-operation tagged-lane snapshots. Public storage/ABI boundaries
remain ordinary inline enums. See the
[implementation assessment](../../../book/src/proposals/std-simd/implementation.md)
and `measurements.json` for retained results and limits.

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
`float_scalar.plenty`, `float_vector.plenty`, `throughput_scalar.plenty` and
`throughput_vector.plenty` the same way and run, for example,
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

On the Ryzen 7 7840HS, final verification after builds stopped gave median
scalar/vector times of 0.821/1.451 ms for the integer recurrence,
3.002/2.771 ms for one floating vector, and 9.897/2.982 ms for four independent
floating vectors (about 3.32 times faster in that throughput case). Instruction
counts per iteration were approximately 13/14, 13/7 and 37/13 respectively.
The JSON retains the earlier run with another host build present as well as
the quiet verification. These measurements support a bounded throughput win,
and show why vector instructions alone do not guarantee one.

## Integer multiplication follow-up

`integer_chains.plenty` compares one and two independent vectors against four
and eight scalar accumulators, with the same runtime multiplier and increment.
The final vector reduction combines lanes before summing. To reproduce the
exit-shape spill, replace its final `v0.wrapping_add(v1).wrapping_sum()` with
`v0.wrapping_sum().wrapping_add(v1.wrapping_sum())`.

Run `python3 integer_chains.py BASELINE_COMPILER SSE41_COMPILER` to compile both
sets and collect seven timing samples plus three counter samples per kernel.
The second compiler used for this investigation was built with only
`isa_builder.set("has_sse41", "true")` added after
`cranelift_native::builder_with_options(false)` in `src/codegen.rs`. That temporary
change was restored; it is not part of Plenty's production configuration.
Only run that experimental compiler on hardware supporting SSE4.1.

`integer-measurements.json` retains both reduction shapes. The
[causal assessment](../../../book/src/proposals/std-simd/integer-multiplication.md)
explains the assembly, cycles, instruction counts and limitations. In the clean
two-vector case baseline SSE2 reaches scalar parity and SSE4.1 is about 1.9 times
faster. One dependent vector remains slower on SSE2 and reaches parity on SSE4.1.

`integer_spill.plenty` retains the original separate-sum exit for issue #444.
Compile it with `--release --compile` for the scalar/vector timing comparison,
or dump `separate_sums` with `--dump clif-opt,asm --dump-filter separate_sums`.
The fixed baseline compiler lowers integer `wrapping_sum` without ABI calls,
so both loop accumulators remain in registers. `spill-measurements.json` compares
the identical source before and after this fix, without enabling a newer ISA.
On the Ryzen 7 7840HS the original separate-sums kernel fell from 3.369 ms to
1.676 ms per million iterations, matching its scalar baseline (1.670 ms).
Median counters fell from 24.03 to 21.03 instructions and 16.19 to 8.12 cycles
per iteration. Compile `integer_spill_counter.plenty` and run it with
`perf stat -e instructions:u,cycles:u` to reproduce the ten-million-iteration
counter probe. These results isolate the reproduced spill, not a general SIMD
speedup or a guarantee about register allocation in other kernels.

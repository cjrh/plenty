# Terminal accessor wrappers (issue #958)

Baseline: `caf7c247`. After: this PR. Both compilers are independent main-based
builds; no #972 changes are included. AMD Ryzen 7 7840HS, x86-64 Linux, GCC
16.2.1 `-O2`; Plenty optimized for baseline x86-64. Kernels and measurements
were isolated before changing compiler code.

The general inlining planner rejected every body containing `TailCall`, even
when native tail lowering substituted its accessor. Thus `Rng.next` and the
independent user-class `Wrapped.terminal` already had cheap standalone bodies,
but callers still paid native calls and frames each iteration.

## Kernel instructions per pass

| Kernel | Before hot loop | Called body, through return | After | C |
| --- | ---: | ---: | ---: | ---: |
| `fill`, bounded field recurrence | 7 | 13 | 12 | 12 |
| `sum_accessor`, checked sum | 8 | 6 | 6 | 5 |

A loop count alone hides out-of-line work; before totals are 20 and 14.
Both lose two argument moves, the call, and five frame/return instructions:
eight per pass. The recurrence retains its arithmetic, two stores, range step
and loop comparison. No overflow/bounds check, negative-index join, header
reload or spill remains in that hot loop. The checked sum retains its overflow
trap and loop checks. C hoists its unmodified field load out of the loop,
accounting for the remaining one-instruction difference. This is the known
invariant-load limitation recorded in closed issue #938; its earlier hoisting
proposal was declined after CPU measurements. No hoisting or backend fork is
changed here. Inlining `fill` inside `main` uses 13/pass because of register
allocation; the standalone kernel is 12. All counts use `loops.py`.

The C seed is unsigned because `Bounded[i64,0,139967]` guarantees a nonnegative
recurrence far from overflow; arithmetic agrees on every valid seed. The sum
uses `__builtin_add_overflow` and traps, matching Plenty's `unchecked:` overflow
behavior rather than silently wrapping C arithmetic.

## IR variants

Run the existing CLIF harness on `variants.clif`.

| Variant | IR change | Hot loop |
| --- | --- | ---: |
| a | Original `sum_accessor` call | 8, plus 6 in callee |
| b | Substitute field load at that call site | 6 |

The compiler change matches b. It admits a wrapper only if it owns no managed
inputs/locals and every direct terminal target is already a leaf substitution
or successfully selected for general inlining under the existing size bounds.
Recursive targets, indirect tails and targets that remain native calls still
exclude the wrapper. Existing tail lowering and cleanup ladders are unchanged.
The managed-callee result path remains valid because its own inlined body owns
that cleanup; a managed outer wrapper is deliberately excluded. This avoids
reintroducing issue #725's duplicated cleanup or changing drop-before-call order.

## Whole fasta program

`fasta_tail.plenty` differs from the current benchmark only in `Rng.next`: it
stores the proved next seed and ends in `self.seed.get()`. No benchmark source
change is required to adopt the compiler fix. Five interleaved rotated-order
runs per executable, input 5,000,000, pinned CPU 3, stdout `/dev/null`, caches
disabled at compilation. All three executables match the checked-in input-1000
fixture exactly. Raw readings are in `runtime-results.json`.

| Executable | Median instructions | Median CPU ms | CPU range ms |
| --- | ---: | ---: | ---: |
| Existing local-return benchmark, baseline | 1,961,159,119 | 309.16 | 308.42–311.82 |
| Terminal-accessor candidate, before | 2,572,780,303 | 349.98 | 346.52–350.51 |
| Terminal-accessor candidate, after | 1,957,818,436 | 311.23 | 309.76–313.86 |

The candidate loses 23.9% of retired instructions and about 11.1% median CPU
against its own baseline. Its instructions match or slightly improve the
existing idiom; overlapping CPU ranges do not establish a difference between
the two idioms. The optimization independently fixes the issue's reproduction
and ordinary user accessors; #972's narrower Bounded representation is separate.

## Compile time

Caches disabled, CPU 3, three runs per build. AOT harness arguments:
`1000 10 --workload aot`, including the usual warmup. Real programs compile
once per sample. Raw readings: `compilebench-results.json`, `compile-results.json`.

| Probe | Before median (range), M instructions | After median (range), M instructions | Change |
| --- | ---: | ---: | ---: |
| 1000-unit AOT harness | 128,713.226 (128,712.364–128,713.917) | 128,797.337 (128,794.755–128,798.698) | +0.065% |
| Spectral-norm | 383.824 (383.817–383.831) | 383.961 (383.954–383.971) | +0.036% |
| Original fasta | 392.283 (392.275–392.286) | 393.526 (393.506–393.527) | +0.317% |
| Terminal-accessor fasta | 392.654 (392.636–392.717) | 393.544 (393.520–393.544) | +0.226% |

## Reproduction

Build each compiler with `cargo build --locked --release --bin plenty --example
compile_bench`. Run:

```sh
PLENTY_CACHE=off plenty --dump ops,clif-opt,asm experiments/kernels/accessor-tail/kernels.plenty -o /tmp/at
python3 experiments/kernels/loops.py < /tmp/at/asm.txt
gcc -O2 -c experiments/kernels/accessor-tail/kernels.c -o /tmp/at.o
objdump -d --no-show-raw-insn /tmp/at.o | python3 experiments/kernels/loops.py
```

Dump standalone methods with `--dump-std --dump-filter Rng.next` or
`--dump-filter Wrapped.terminal` to count called instructions through `ret`.
Build the CLIF harness using its existing README, then run it on `variants.clif`.
Compile both fasta sources with each compiler, `PLENTY_CACHE=off plenty --compile
SOURCE -o EXECUTABLE`. Measure using `perf stat -x, -e instructions:u,task-clock
taskset -c 3 EXECUTABLE 5000000 >/dev/null`, rotating executable order. Run
`PLENTY_CACHE=off perf stat -x, -e instructions:u taskset -c 3 compile_bench
1000 10 --workload aot` for the compiler probe; use the same `perf` wrapper on
`plenty --compile SOURCE -o EXECUTABLE` for real compilation measurements.

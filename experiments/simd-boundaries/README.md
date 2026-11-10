# Native SIMD construction and tuple projection

Compiler delivery for issue #929, based on main `56365416`, x86-64 Linux,
Ryzen 7 7840HS, Cranelift 0.137.0, GCC 16.2.1. Optimized builds only.
This changes reusable lowering, not the n-body benchmark source or its algorithm.

## Kernel and attribution

`projection.plenty` repeatedly adds two lanes, unpacks them and accumulates their
product. `projection.c` expresses the same recurrence with baseline SSE2.
`projection.clif` tests native construction/extraction before implementing the
compiler change. No fast math, reassociation or newer ISA is enabled.

| Build | Executed instructions per steady iteration |
|---|---:|
| Plenty before | 129 including caller-context helpers |
| Plenty after | 9 |
| GCC -O3 | 9 |
| Direct CLIF prototype | 11 |

`experiments/kernels/loops.py` gives a 112-instruction pre-change caller span.
Its union includes four instructions on the unused null-storage branch
(0x2cb, 0x2ce, 0x2d6, 0x2de); the actual non-null path has 108. The two context
helpers account for the remaining 21 instructions measured per iteration.
The final loop has no calls, loads or stores.

| Cause | Before | After |
|---|---:|---:|
| Vector addition, high-lane extraction, multiply, accumulate | 4 | 4 |
| Counter/control flow | 5 | 4 |
| Representation guards | 8 | 0 |
| Caller-context call instructions | 2 | 0 |
| Storage/address/representation movement | 89 | 0 |
| Instructions inside caller-context helpers | 21 | 0 |
| SSE operand copy | 0 | 1 |
| Total | 129 | 9 |

C uses two operand copies and three counter/control instructions; Plenty uses
one copy and four counter/control instructions, including its unused source
range index update. The direct CLIF version uses an extra register transfer and
back-edge jump. Residual empty snapshot stack slots still reserve frame space
(304 bytes in the small kernel); eliminating these is outside this change.

The implementation recognizes the exact trusted numeric SIMD surface, records
native constructors and `to_tuple()` results, and extracts split fields directly.
Tuple destructuring introduces a StoreLocal/MoveLocal temporary. Matching numeric
tuple locals used by splits and without borrows are promoted to vector SSA so
that this temporary does not undo the optimization. Native values retain lazy
canonical snapshots for ABI/observable uses; existing snapshot liveness removes
unused stores. Mutable-borrowed vectors and borrowed tuples use ordinary storage.
Moving/emptying a promoted tuple clears its native variable instead of reading
a null canonical pointer. No global inlining budget or dependency is changed.

## Whole programs

One million iterations/steps, pinned core 0, three instruction observations and
nine timing rounds in rotating order. Timings include startup and were collected
on a shared machine; [counts.json](counts.json) retains every sample and range.

| Source | Before M instructions | After M instructions | Before median ms | After median ms |
|---|---:|---:|---:|---:|
| Projection | 129.634 | 9.634 | 11.64 | 2.78 |
| n-body fixed x/y SIMD | 2,024.686 | 545.684 | 253.75 | 51.12 |
| n-body paired magnitudes | 1,383.684 | 646.684 | 241.62 | 52.00 |
| Original list-based n-body | 1,995.708 | 1,995.708 | 113.33 | 111.25 |
| C projection | 9.463 | 9.463 | 2.62 | 2.62 |

The unchanged x/y source now executes 545 loop instructions, versus the
493-instruction baseline direct-CLIF feasibility target. The earlier Rust
baseline was 451.635M instructions and native Rust 229.635M; those are historical
instruction comparisons, not same-session timings. The larger automatic
aggregate/loop transformation (issue #930), list transfers and wider SIMD
boundaries (issue #929) remain. The original benchmark has no instruction gain.

Before/after output matches for -1, 0, 1, 2, 3, 10, 1,000, 10,000 and 1,000,000
steps: 36 program/step comparisons, including all 35 n-body state fields and two
energy lines. C projection results match as exact floating-point values.
Behavior tests cover signed/unsigned lanes, float edge values, tuple mutation,
snapshots, joins and escaping tuples in both compilation profiles. An emitted-code
regression requires a packed addition without calls or stack traffic.

Uncached whole-program compilation, including child processes:

| Source | Before M instructions (range) | After M instructions (range) |
|---|---:|---:|
| Projection | 151.428 (151.422–151.436) | 148.517 (148.509–148.523) |
| x/y n-body | 598.838 (598.819–598.901) | 569.718 (569.695–569.772) |
| Original n-body | 504.011 (503.988–504.053) | 504.119 (504.103–504.136) |

The release `compile_bench 1000 10 --workload aot` run used 127.846 billion
user instructions including warmup and linking; median AOT time was 1,015.18 ms.
This is an after-only compiler-throughput observation, not a claimed speedup.
The per-program paired counts above provide the regression comparison.

## Reproduction

From the task worktree, save the baseline release compiler as
`target/simd-boundaries/plenty-before` before applying the change. Build the
modified release compiler and the kernel harness using the commands in
[the kernel probes](../kernels/README.md).

```sh
python3 experiments/nbody-structural/generate.py target/simd-boundaries/nbody
gcc -O3 -c experiments/simd-boundaries/projection.c -o target/simd-boundaries/c.o
gcc -O3 experiments/simd-boundaries/driver.c target/simd-boundaries/c.o -o target/simd-boundaries/projection-c
objdump -d --no-show-raw-insn target/simd-boundaries/c.o > target/simd-boundaries/c.asm
target/release/plenty-kernel-harness experiments/simd-boundaries/projection.clif > target/simd-boundaries/clif.asm
```

Compile `projection.plenty` from this directory and `simd-xy.plenty`,
`simd-locals.plenty`, `original.plenty` from the generated directory with each
compiler, using `--no-cache --compile SOURCE -o target/simd-boundaries/NAME-before`
and `NAME-after` respectively. Dump `clif-opt,asm` with `--dump-filter projection`
or `advance`; feed the assembly to `experiments/kernels/loops.py`.
Then run `python3 experiments/simd-boundaries/measure.py` to verify results,
collect hardware counts, alternating timings and compilation counts.

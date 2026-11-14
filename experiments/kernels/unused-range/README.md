# Unused range elements (issue #973)

`kernels.plenty` isolates an unused dynamic range element and Mandelbrot's
escape loop. `kernels.c` does the same work with the same scalar types and
floating-point order. The escape condition uses `<=` in both languages,
including the same result for unordered values. This kernel keeps its pre-update
check to isolate the lowering change. The benchmark tests for escape after each
update: its initial magnitude is zero, so the first update always runs. It still
returns false for NaN inputs; those may finish all 50 bounded updates.

Optimized x86-64 baseline ISA, Cranelift 0.137.0 at Plenty's locked fork revision,
gcc 16.2.1 `-O2 -fno-tree-loop-distribute-patterns`. Before = `9dee6930`.

| Kernel | C | Plenty before | Plenty after |
| --- | ---: | ---: | ---: |
| `unused` | 5 | 6 | 5 |
| `escape` | 15 | 21 | 20 |

Counts are instructions per steady-state pass from `../loops.py`. The
standalone and inlined forms have equal counts. The floating recurrence
executes two useful instructions; the escape recurrence executes eight.

## Attribution

| Cause | `unused` before / after | `escape` before / after | C `escape` |
| --- | ---: | ---: | ---: |
| Floating work | 2 / 2 | 8 / 8 | 8 |
| Scalar register copies | 0 / 0 | 6 / 6 | 3 |
| Countdown/index control | 3 / 3 | 3 / 3 | 2 |
| Escape comparison and branch | 0 / 0 | 2 / 2 | 2 |
| Unconditional back jump | 0 / 0 | 1 / 1 | 0 |
| Unused element increment | 1 / 0 | 1 / 0 | 0 |

No bounds, overflow, negative-index, header, spill, or call instructions occur
on these hot paths. This change removes exactly the unused increment and its
initialization/register; it does not alter arithmetic checks or floating work.
The retained scalar-copy and early-exit loop-layout costs are issues #978 and
#977 respectively.

`variants.clif` reduces the optimized `unused` dump to a single loop:

| Variant | IR shape | Instructions per pass |
| --- | --- | ---: |
| a | Dead element is still a cyclic block parameter | 6 |
| b | Omit element stores, breaking the recurrence | 5 |

Cranelift's optimized IR retains the dead cyclic parameter in a. Suppressing
its stores in Plenty lets ordinary DCE remove its loads and wrapping addition.
The proof requires a recognized direct 64-bit unit-step range, validated
length/start/step entry values, no element reads outside its wrapping steps,
and no element borrowing or other observation. Named unused elements qualify
as well as `_`. Stored, reversed, narrow and non-unit-step ranges retain
existing lowering. Manual `while` counters remain observable after their loop.

## Reproduce

Build the compiler in the checkout being measured. For the before comparison,
build `9dee6930` in a separate worktree and pass it these retained kernel files.
Use separate dump directories and executable names for each build.

```sh
RUSTC_WRAPPER= cargo build --locked --release --bin plenty
PLENTY_CACHE=off target/release/plenty --dump clif-opt,asm \
    experiments/kernels/unused-range/kernels.plenty -o /tmp/unused-range
python3 experiments/kernels/loops.py < /tmp/unused-range/asm.txt

gcc -O2 -fno-tree-loop-distribute-patterns \
    -c experiments/kernels/unused-range/kernels.c -o /tmp/unused-range-c.o
objdump -d --no-show-raw-insn /tmp/unused-range-c.o \
    | python3 experiments/kernels/loops.py

RUSTC_WRAPPER= cargo build --locked --release \
    --manifest-path experiments/kernels/harness/Cargo.toml --target-dir target
target/release/plenty-kernel-harness \
    experiments/kernels/unused-range/variants.clif \
    | python3 experiments/kernels/loops.py

PLENTY_CACHE=off target/release/plenty --compile \
    benchmarks/game/mandelbrot/plenty/mandelbrot.plenty -o /tmp/mandelbrot
perf stat -e instructions:u /tmp/mandelbrot 4000 > /tmp/mandelbrot.pbm
```

Compare the bitmap byte-for-byte across before/after and the C reference.
Compile-time measurements use `PLENTY_CACHE=off` and `instructions:u`, including
archive extraction/linking; wall-clock measurements on this shared machine are
not performance evidence.

## Whole program and compilation

Mandelbrot at 4000, `perf stat -e instructions:u`, three runs per executable:

| Build/source | Median instructions | Min–max |
| --- | ---: | ---: |
| Before compiler, original `while` | 8,269,219,055 | 8,269,219,010–8,269,219,105 |
| Before compiler, final post-update `for` | 8,598,564,233 | 8,598,564,199–8,598,564,240 |
| After compiler, final post-update `for` | 8,201,704,140 | 8,201,703,976–8,201,704,219 |
| C reference, gcc `-O2` | 6,288,683,252 | 6,288,683,091–6,288,683,450 |

The compiler change removes 4.62% for identical `for` source. The delivered
rewrite uses 0.82% fewer instructions than the original `while`. Outputs are
byte-identical at sizes 1, 7, 8, 9, 31, 200 and 4000, including padded rows.
An initial pre-update-guard rewrite cost 5.46% on the old compiler and retained
0.54% overhead after the fix; the post-update escape check avoids that startup
cost. All changes preserve the recurrence's FP arithmetic order.

Compiler instructions with `PLENTY_CACHE=off`, including extraction/linking:

| Input | Before median (range) | After median (range) |
| --- | ---: | ---: |
| These kernels, 3 runs | 120.919M (119.804–121.504M) | 119.706M (119.389–119.826M) |
| Mandelbrot, identical final source, 3 runs | 335.475M (334.339–340.371M) | 337.851M (335.391–338.197M) |
| `compile_bench 1000 10 --workload aot`, 2 runs | 128.33244B (128.33106–128.33381B) | 128.32850B (128.32692–128.33007B) |

The larger AOT probe changes by less than 0.004%; the smaller link-heavy
compilations overlap their run spread. There is no established compiler-speed
change. For the AOT probe, the unchanged benchmark driver was compiled with
`rustc -Copt-level=3` against each worktree release library and the same
allocator/dependencies; this avoided an unrelated Cargo build while retaining
its ten repetitions and one warm-up. The standalone compiler binaries are
normal `cargo build --locked --release --bin plenty` builds.
Raw samples are retained in `results.json`.

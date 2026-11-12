# Typed float loads (issue #812)

> The `--unchecked assume` build mode used in this record was removed. The
> measurements are historical; commands that pass `--unchecked` no longer run.

Scalar `f32`/`f64` reads from list rows, views, references and class fields
used to load an integer word and `bitcast` it, which x86-64 lowers to an
integer `mov` plus a GPR-to-XMM `movq`/`movd`. They now load the float type
directly. `kernels.plenty` holds three summing loops and two single reads;
`kernels.c` has the same loops. The gather kernel is
[`../unchecked-index/kernels.plenty`](../unchecked-index/kernels.plenty),
compiled with `--unchecked assume`.

Optimized x86-64 Linux builds, Plenty before = `30ecb2c1`, Cranelift 0.137.0,
gcc 16.2.1 `-O2 -fno-tree-loop-distribute-patterns`. Counts are instructions
per hot pass from [`loops.py`](../loops.py); the two `sum_index`/`sum_f32`
rows are the checked and the proven loop versions.

| Kernel | C | Before | After |
| --- | --- | --- | --- |
| `sum_index` (f64, indexed) | 5 | 9 / 7 | 8 / 6 |
| `sum_iter` (f64, `for x in xs`) | 5 | 6 | 5 |
| `sum_f32` (f32, indexed) | 5 | 9 / 7 | 8 / 6 |
| `gather` (`xs[indices[i]]`, assumed) | 5 | 8 | 7 |

Attribution of the change: each loop loses its `movq`/`movd`; the integer
`mov` load becomes `movsd`/`movss`. The single reads (`read_ref`, `field`)
lose the same move.

The load is still not folded into `addsd` as a memory operand, which would
remove one more instruction per pass. Two causes, the second sufficient by
itself:

1. A rotated loop's latch is a separate block (issue #726's layout), so the
   egraph places the `fadd` in the latch while the side-effecting load stays in
   the body block; x64 lowering merges loads only within a block.
2. The `fadd` result is the loop-carried value and also the exit value, so it
   has two uses (`brif v82, block4(v43, ...), block1(v43)`). Cranelift's
   `ValueUseState` makes `Multiple` contagious to operands
   (`cranelift-codegen` 0.137.0 `src/machinst/lower.rs`), so the load counts as
   used more than once and is not sunk.

| CLIF variant | Change | Instructions per pass |
| --- | --- | --- |
| `gather-lowered.clif` | Actual updated lowering | 7 |
| `gather-merged-latch.clif` | Latch merged into the body block | 7 |
| [`../unchecked-index/float-load.clif`](../unchecked-index/float-load.clif) | Older unrotated loop; exit reads the header's parameter, so the sum has one use | 7, with `addsd (%rdi,%rax,8)` folded |

Merging the latch alone gains nothing, so the remaining fold is a Cranelift
lowering heuristic, not a Plenty change. Issue #853's upstream write-up,
[`upstream-use-state.md`](upstream-use-state.md), has a minimal CLIF
reproduction ([`use-state.clif`](use-state.clif)), a proposed Cranelift change
([`use-state.patch`](use-state.patch)), its effect on these kernels with and
without a merged latch, and the Plenty-side workarounds that do not help.

Whole programs, `instructions:u`, identical output:

| Program (argument) | Before | After | Change |
| --- | --- | --- | --- |
| n-body `nbody.plenty` (5000000) | 11 795 334 288 | 10 240 334 006 | -13.2% |
| n-body `nbody_unchecked.plenty` (5000000) | 11 605 333 997 | 9 990 333 363 | -13.9% |
| n-body `nbody_clist.plenty` (5000000) | 26 415 319 528 | 25 765 319 593 | -2.5% |
| spectral-norm (2000) | 7 204 637 412 | 7 044 633 408 | -2.2% |
| mandelbrot (4000) | 10 473 187 924 | 10 473 188 359 | 0 |

```sh
target/release/plenty --dump asm experiments/kernels/float-loads/kernels.plenty -o /tmp/f
# Normalize as in ../README.md, then:
python3 experiments/kernels/loops.py < /tmp/f/asm.normalized
gcc -O2 -fno-tree-loop-distribute-patterns -c experiments/kernels/float-loads/kernels.c -o /tmp/f.o
objdump -d --no-show-raw-insn /tmp/f.o | python3 experiments/kernels/loops.py
target/kernel-harness/release/plenty-kernel-harness \
  experiments/kernels/float-loads/gather-merged-latch.clif | python3 experiments/kernels/loops.py
```

To measure the patch, copy `cranelift-codegen` 0.137.0 from the Cargo
registry, apply `use-state.patch` in the crate directory with `patch -p3`
(its paths are relative to the Wasmtime repository), and build a copy of the
harness with `[patch.crates-io] cranelift-codegen = { path = ... }`.

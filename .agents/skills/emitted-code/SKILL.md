---
name: emitted-code
description: For maintenance of the Plenty compiler repository. Analyze and speed up the machine code Plenty generates - bounds checks, overflow checks, negative-index normalization, register moves, loop layout, inlining - by isolating small kernels, comparing them instruction by instruction with C, testing Cranelift IR variants, and attributing every extra instruction before changing the compiler. Use for any performance work on generated programs or benchmarks (not compiler build speed, which is profiling.md).
---

# Analyze emitted code

The procedure lives in `book/src/development/emitted-code.md`. Read it first
and follow its steps in order; this skill only lists what to have ready.

1. Build the compiler in your worktree (`cargo build --locked --release --bin plenty`)
   and measure optimized Plenty builds only; never a `--fast-compile` build.
2. Count with `perf stat -e instructions:u` and per-pass counts from
   `experiments/kernels/loops.py`; treat wall-clock time on a shared machine as
   noise.
3. Start from `experiments/kernels/` (kernels in Plenty and C, `loops.py`, the
   CLIF harness, and its README) rather than writing new tooling.
4. Remove a check only when the compiler proves it cannot fail; keep trap
   kinds, positions and messages for checks that can.
5. Report with the template at the end of the procedure page: kernel table,
   attribution by cause, IR variant table, whole-program and compile-time
   counts. File issues for causes left unfixed. A regalloc2 or Cranelift
   cause may be fixed in the project's forks (`../regalloc2`, `../wasmtime`,
   branch `fork`) by the procedure in
   `book/src/development/workflow.md#cranelift-and-regalloc2-forks`; keep the
   write-up for an upstream report.

# Incrementing a checked index (issue #930)

> The `--unchecked assume` build mode used in this record was removed. The
> measurements are historical; commands that pass `--unchecked` no longer run.

A successful list/view row check proves the original `i64` index is less than
`i64.MAX`, including when it is negative. Lengths fit in `i64`; positive
`i64.MAX` cannot pass the check. Therefore adding one cannot overflow. This is
a fact about the checked SSA value, not a mutable source variable or a loop
that may not execute. The existing bounds-check cache's dominance chain scopes
the proof; joins and different SSA values do not inherit it. No bounds check
is removed, and unchecked-assume indexing records no such evidence.

Both ordinary trapping arithmetic and Result-returning arithmetic share the
proof in `compared_add_sub`. It only applies to signed 64-bit addition of one.
Other increments, widths and operations retain their existing checks.

## Instruction evidence

The [velocity kernel](../nbody-codegen/velocity.plenty) originally had an
avoidable `seto`, `test`, `jne` after its `k += 1`. Four scratch-list checks
already prove that increment safe. Transforming only its two checked
increments in the optimized CLIF produced this result with the existing
Cranelift harness, before changing compiler lowering:

| Shape | Fast path instructions/pair | Fallback instructions/pair |
|---|---:|---:|
| Original CLIF | 62 | 70 |
| Plain-add IR probe | 59 | 67 |
| Compiler proof | 59 | 67 |
| Scalar GCC -O2 reference | 34 | — |

All three removed instructions report overflow. The existing ledger's other
25 excess instructions remain: bounds 8, spills 6, address work 5, loop control
2, FP loads/copies 4. The compiler reproduces the best IR variant exactly;
there is no dependency patch or relaxed safety contract.

At one million n-body steps, [three runs](counts.json) changed from
2,049,712,918–2,049,713,319 instructions to 1,995,712,842–1,995,712,965, about
2.63% fewer. Outputs match byte-for-byte. The proof also affects other checked
index increments in the full benchmark; the gain is not only the velocity
loop's 30 million removed instructions.

Nine alternating pinned-core [timings](timings.json) had medians 0.1141s before
and 0.1111s after, with ranges 0.1102–0.1168s and 0.1099–0.1192s. The median
moves in the expected direction; overlapping ranges on the shared machine
limit confidence in the precise time improvement. Cache-disabled n-body
[compile instructions](compile-counts.json), including linker work, span
469.9–484.0M before and 469.2–476.5M after. Do not infer a compiler speedup from
that spread. The original primary-worktree release compiler supplied baseline
programs; this worktree's release compiler supplied the changed programs.

## Reproduce

Dump the baseline kernel, convert it with [variants.py](variants.py), and run
the existing harness:

```sh
PLENTY_CACHE=off BEFORE_PLENTY --dump clif-opt,asm --dump-filter velocity \
  experiments/nbody-codegen/velocity.plenty -o target/velocity-before
python3 experiments/index-increment/variants.py \
  target/velocity-before/clif-opt.txt target/velocity-ir
TARGET_HARNESS target/velocity-ir/before.clif target/velocity-ir/after.clif \
  > target/velocity-ir/variants.txt
python3 experiments/kernels/loops.py < target/velocity-ir/variants.txt
```

Build the changed release compiler and dump the same kernel. Compile the
unchanged n-body benchmark with each compiler, then run `perf stat -e
instructions:u taskset -c 0 PROGRAM 1000000`; compare output as well as counts.

The focused regression test checks both profiles, successful negative and
nonnegative indices, branch joins, a changed index, and out-of-bounds indices
including both integer extrema. Dump checks require the overflow instruction
to disappear only when the bounds check dominates it.

Issue #930 remains open for the scratch-buffer bounds, register pressure,
address work and surrounding-loop overhead. This delivery does not attempt a
general triangular-loop proof or fixed-size scalar replacement.

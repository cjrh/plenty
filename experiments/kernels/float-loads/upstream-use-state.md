# Cranelift: a load feeding a value used twice as block arguments is not sunk

Draft report for Cranelift (issue #853). Not filed; check current Cranelift
`main` before filing, since this was found on 0.137.0.

## Summary

x64 lowering does not fold a load into its only user when that user's result
is passed as a block argument more than once. Every rotated (do-while)
reduction loop has that shape: the sum goes to the back edge and to the exit.

```text
block1(v10: f64, v11: i64, v12: i64):
    v13 = load.f64 notrap aligned v11
    v14 = fadd v10, v13
    ...
    brif v17, block1(v14, v15, v16), block2(v14)
```

gives

```text
movsd (%rdi), %xmm1
subq  $1, %rsi
addsd %xmm1, %xmm0
addq  $8, %rdi
testq %rsi, %rsi
jg    loop
```

Passing anything else to `block2` gives `addsd (%rdi), %xmm0` and one fewer
instruction per pass. gcc -O2 folds the load in both cases.

## Cause

`compute_use_states` in `cranelift-codegen/src/machinst/lower.rs` counts
every operand, block-call arguments included. `v14` has two uses, so it is
`Multiple`, and step 2 propagates `Multiple` to its operands: `v13` becomes
`Multiple` although `fadd` is its only user. `get_value_as_source_or_const`
then refuses to sink the side-effecting load, which needs `Once`.

The propagation exists because a consumer may merge an operand's whole
instruction tree into its own lowering, so a value with two merging users
could have its tree emitted twice. Block-call arguments never merge: the
branch puts each argument in a register, and `v14`'s `fadd` is lowered once
at its definition. Its operand tree is therefore lowered once however many
block arguments name `v14`.

## Proposed change

Propagate multiplicity only from values with at least one use as an
instruction argument (`inst_args`, which excludes block-call arguments). The
diff is [`use-state.patch`](use-state.patch); `cranelift-codegen`'s 200 unit
tests pass with it. Return values and call arguments still count as merging
uses, which is conservative.

## Reproduction

[`use-state.clif`](use-state.clif) holds the loop above (`%exit_sum`) and the
same loop exiting with the counter (`%exit_count`). Compiled with Plenty's
release flags (`opt_level=speed`, x86-64 baseline) by the
[kernel harness](../harness/), instructions per pass:

| Function | 0.137.0 | With the patch |
| --- | --- | --- |
| `%exit_sum` | 6 | 5 |
| `%exit_count` | 5 | 5 |

## Effect on Plenty's kernels

The fold also needs the load and the `fadd` in one block, since sinking stops
at block boundaries. Plenty's rotated loops test the condition in a separate
latch block, and the egraph elaborates the `fadd` there, next to its uses. The
`*-merged.clif` files are Plenty's dumps with that latch merged into the body,
which Plenty could do when the latch has no `continue` predecessor.
Instructions per pass (checked / proven loops where there are two):

| Kernel | C | 0.137.0 | Patch only | Merged latch only | Both |
| --- | --- | --- | --- | --- | --- |
| `gather` (`gather-*.clif`) | 5 | 7 | 7 | 7 | 6 |
| `sum_index` | 5 | 8 / 6 | 8 / 6 | 8 / 6 | 7 / 5 |
| `sum_f32` | 5 | 8 / 6 | 8 / 6 | 8 / 6 | 7 / 5 |
| `sum_iter` | 5 | 5 | 5 | 5 | 6 |

`sum_iter` loses one: its index both addresses the element and is
incremented. The egraph elaborates the `brif`'s operands in order, so the
increment and compare come before the `fadd`; once the load moves into the
`addsd`, the old and new index are live together and regalloc2 splits the
back edge with a `mov` and a `jmp`. Without the fold the load stays at its
original position, before the increment. A report should mention this
interaction; Plenty would need to check every loop shape before relying on
the change.

## Workarounds evaluated in Plenty

None removes the instruction with Cranelift 0.137.0:

- Exit from the header, so the exit reads the header's parameter
  ([`../unchecked-index/float-load.clif`](../unchecked-index/float-load.clif)):
  the load folds, but that is the unrotated loop, which pays a `jmp` per pass;
  7 instructions, as the rotated loop. Rotation (issue #726) and the fold save
  the same instruction.
- A bottom test block that receives the loop values as parameters, so the
  `fadd`'s only use is the body's `jump`
  ([`gather-latch-params.clif`](gather-latch-params.clif)): the mid-end
  removes the parameters of a single-predecessor block and the `fadd` is
  placed in that block again; 7.
- Merging the latch alone ([`gather-merged-latch.clif`](gather-merged-latch.clif)):
  the sum still has two uses; 7.

Plenty keeps the separate latch until an upstream change lands; merging it is
then the Plenty-side half of the fix.

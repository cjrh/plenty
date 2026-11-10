# Indexed loop conditions and unit-step range loops

The cumulative-limit search of the fasta program in `benchmarks/game/`
(`while limits[k] < r: k += 1`) over a view and a borrowed list, the same
search inside a counted loop, and a unit-step `range` loop, as standalone
functions in `kernels.plenty` and `kernels.c` (issues #932 and #933).

Optimized x86-64 Linux builds, Plenty before = `f0a789a7`, Cranelift 0.137.0,
gcc 16.2.1 `-O2 -fno-tree-vectorize`. Counts are instructions per pass of the
innermost loop from [`loops.py`](../loops.py).

| Kernel | C | Before | After |
| --- | --- | --- | --- |
| `find_view` | 3 | 10 | 6 |
| `find_list` | 3 | 8 | 6 |
| `select_all`, search loop | 3 | 9 | 7 |
| `total` | 4 | 9 | 8 |

```sh
cargo build --locked --release --bin plenty
target/release/plenty --release --dump ops,clif-opt,asm \
    experiments/kernels/indexed-conditions/kernels.plenty -o /tmp/ic
python3 experiments/kernels/loops.py < /tmp/ic/asm.txt
gcc -O2 -fno-tree-vectorize -c experiments/kernels/indexed-conditions/kernels.c -o /tmp/ic.o
objdump -d --no-show-raw-insn /tmp/ic.o | python3 experiments/kernels/loops.py
```

## Attribution

`find_view`, 10 to 6 (`find_list` is the same loop after its header reads):

| Cause | Before | After |
| --- | --- | --- |
| Overflow check on `k += 1` (`seto; test; jne`) | 3 | 0 |
| Unrotated loop (`jmp`) | 1 | 0 |
| Bounds check (`cmp; jae`; C has none) | 2 | 2 |
| Load not folded into the compare | 1 | 1 |
| Useful work (also in C) | 3 | 3 |

The index passed the bounds check of the condition, so it is below a length
and `k + 1` cannot overflow. Before, the view form had no counter: the
analysis looked for a compare of the index with a bound. The loop is rotated
because the condition reads only scalars and the cached length and rows.

The after loop is `add; cmp len; jae; mov (rows,k,8); cmp; jl`. Cranelift
0.137 does not fold a load into the compare of a branch: when it lowers a
block's branch no instruction color is current, so `cmp (mem), reg` is never
chosen there. This is an upstream limit (issue #935).

`select_all`, 9 to 7: as above, minus the header. Before, `limits`' length
and rows were read before the inner loop on every outer pass (4 instructions
per outer pass); now they are read once before the outer loop. The remaining
extra instruction reloads the loop-invariant `targets[i]` on each pass:
Cranelift has no loop-invariant code motion for loads (issue #938). The second copy of
the versioned outer loop has 8: regalloc2 keeps `k + 1` in a second register
for the trap path.

`total`, 9 to 8:

| Cause | Before | After |
| --- | --- | --- |
| Trip counter beside the index (`sub; test; jg`, `add`) | 4 | 3 |
| Overflow check on the sum (`seto; test; jne`, issue #934) | 3 | 3 |
| Useful work | 2 | 2 |

A range with step 1 runs while its variable is below the stop, so the loop
tests `add; cmp; jl` and the countdown is gone. `a_times_u` in
[`value-bounds`](../value-bounds/README.md) goes from 15 to 14 for the same
reason, and its `select` search loop from 8 to 6.

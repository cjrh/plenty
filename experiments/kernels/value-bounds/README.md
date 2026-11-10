# Value bounds, positive divisors and loop conditions

The hot loops of the spectral-norm, mandelbrot and fasta programs in
`benchmarks/game/`, as standalone functions in `kernels.plenty` and
`kernels.c`.

Optimized x86-64 Linux builds, Plenty before = `28cb7214`, Cranelift 0.137.0,
gcc 16.2.1 `-O2 -fno-tree-vectorize`. Counts are instructions per pass of the
innermost loop from [`loops.py`](../loops.py).

| Kernel | C | Before | After |
| --- | --- | --- | --- |
| `a_times_u` (spectral-norm) | 12 | 34 | 15 |
| `escape` (mandelbrot) | 15 | 26 | 22 |
| `select`, search loop (fasta) | 3 | 11 | 8 |

```sh
cargo build --locked --release --bin plenty
target/release/plenty --release --dump ops,clif-opt,asm \
    experiments/kernels/value-bounds/kernels.plenty -o /tmp/vb
python3 experiments/kernels/loops.py < /tmp/vb/asm.txt
gcc -O2 -fno-tree-vectorize -c experiments/kernels/value-bounds/kernels.c -o /tmp/vb.o
objdump -d --no-show-raw-insn /tmp/vb.o | python3 experiments/kernels/loops.py
```

## Attribution

`a_times_u`, 34 to 15:

| Cause | Before | After |
| --- | --- | --- |
| Overflow checks on the five sums (`seto; test; jne` each) | 15 | 0 |
| Overflow check on the product (`jo`; it can overflow) | 1 | 1 |
| Register moves | 2 | 0 |
| Separate trip counter beside the index (`sub; test`) | 2 | 2 |
| Useful work (also in C) | 14 | 12 |

The sums are bounded because `len(u)` of a `&[f64]` is at most `i64.MAX / 8`.
Unchecked sums also fold into `lea`, which removes the moves and two adds.

`escape`, 26 to 22:

| Cause | Before | After |
| --- | --- | --- |
| Overflow check on `i += 1` (`seto; test; jne`) | 3 | 0 |
| `4.0` rebuilt with `movabs` + `movq` | 2 | 0 |
| Register moves (`movdqa`; C has 3) | 7 | 8 |
| `jmp` (before: to the header of an unrotated loop; after: behind the latch's moves) | 1 | 1 |
| Useful work (C: 12, counting down with `sub; je`) | 13 | 13 |

The comparison constant now occupies a register across the loop, and the
allocator answers with more moves in this kernel. The whole program gains
more: mandelbrot 2000 runs 25.5% fewer instructions, 7.0 points of that from
the constant. The moves and the latch `jmp` are regalloc2 and
Cranelift block-layout results, as in issue #720.

`select` search loop `while limits[k] < state: k += 1`, 11 to 8:

| Cause | Before | After |
| --- | --- | --- |
| Overflow check on `k += 1` (`seto; test; jne`) | 3 | 0 |
| Bounds check (`cmp; jae`; C has none) | 2 | 2 |
| Address `lea` not folded into the load, load not folded into the compare | 2 | 2 |
| Unrotated loop (`jmp`) | 1 | 1 |
| Useful work | 3 | 3 |

`k` passed the index check of a list of at most `i64.MAX / 8` rows, so
`k + 1` cannot overflow. The loop stayed unrotated because its condition
indexes a list; issue #932 rotated it later
(see [`indexed-conditions`](../indexed-conditions/README.md)).

The generator step `(state * 3877 + 29573) % 139968` before the loop went
from a sign-compare-and-select fixup to `sar; and; add`. Its addition keeps a
`seto; test; jne` check: Cranelift 0.137 fuses the overflow flag into the
branch for `smul_overflow` and `uadd_overflow` only (see `compared_add_sub`
in `src/codegen.rs`; issue #934). The trip counter of `a_times_u` was
removed later by issue #933.

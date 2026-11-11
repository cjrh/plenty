# Clist field access kernels

Issue #759: isolate `rows[i].field` in loops over a `clist`. `kernels.plenty`
holds n-body's velocity update (an exclusive clist, with `i` from the outer
loop and `j` from the inner one), its position update (`range(len(bodies))`),
and its potential-energy pair loop (a shared clist). `kernels.c` runs the same
loops over plain column arrays.

Build the Plenty compiler in this worktree with
`cargo build --locked --release --bin plenty`, then:

```sh
PLENTY_CACHE=off target/release/plenty --dump ops,clif,clif-opt,asm \
    experiments/kernels/clist/kernels.plenty -o /tmp/clist-kernel

gcc -O2 -fno-tree-loop-distribute-patterns -fno-tree-vectorize -fno-math-errno \
    -c experiments/kernels/clist/kernels.c -o /tmp/clist-kernel-c.o
objdump -d --no-show-raw-insn /tmp/clist-kernel-c.o | python3 experiments/kernels/loops.py
```

Normalize the Plenty assembly for `loops.py` using the command in the
[parent README](../README.md). The program prints
`10 4.080700 -0.800000 -15.419134` with both compilers.

## Recorded result

Measured on 2026-11-07, baseline `30ecb2c1`, AMD Ryzen 7 7840HS, x86-64 Linux,
rustc 1.99.1, pinned Cranelift 0.137.0 and gcc 16.2.1. Instructions per pass
of each inner loop:

| Kernel | C | Before | After |
|---|---:|---:|---:|
| velocity (14 field accesses) | 32 | 307 | 93 (84 with #812) |
| positions (9 field accesses) | 15 | 132 | 19 |
| potential (8 field reads) | 19 | 121 | 30 |

Before, each access loaded the owner pointer, tested it for null, loaded the
row count, normalized a negative index (`lea`, `test`, `mov`, `cmov`),
compared it with the count, and computed the column address from the
capacity: 10 to 12 instructions before the load or store. Now the loop reads
the row count and each column address once, before it starts; the first
access to a row checks the index and later fields of that row reuse the check.
`positions` has no check left: its index is below `len(bodies)`, which is the
cached row count. Its remaining four instructions are a register copy of `dt`
for each field and a separate trip counter.

The velocity loop's remaining instructions over C are not clist header
work: four list bounds checks on `k` (`lea`, `cmp`, `jae`), the inner-loop
check of the outer index `i`, the overflow check of `k += 1`, and spills of
the column addresses, which exceed the free registers. Before issue #812 the
list floats also loaded through an integer register, which cost nine more
instructions per pass.

Issue #943 moved the velocity kernel's header reads before the outer loop.
Measured on 2026-11-10 against `2d20ddf5`: 70 instructions per pass before,
58 in a copy with four of the six bounds checks, which the fixed row count
allows, and 66 in the fully checked copy. The 58-instruction copy is the one
that runs: with breakpoints on both back edges, the kernel takes the first
six times (ten passes in four entered inner loops) and the second never. `positions` stays at 18; `potential` goes from 29 to 24
and 28.

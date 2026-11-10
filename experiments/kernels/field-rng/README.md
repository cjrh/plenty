# A generator held in a class field

The linear congruential generator of the fasta program in `benchmarks/game/`:
a class field updated through a `&mut` receiver by a small fallible method,
called with `?` in a loop (`fill`), and the same recurrence on a local
(`fill_local`), in `kernels.plenty` and `kernels.c` (issues #936 and #937).

Optimized x86-64 Linux builds, Plenty before = `5f43644d`, Cranelift 0.137.0,
gcc 16.2.1 `-O2 -fno-tree-vectorize`. Counts are instructions per pass of the
loop from [`loops.py`](../loops.py).

| Kernel | C | Before | After |
| --- | --- | --- | --- |
| `fill` | 15 | 35 | 29 |
| `fill_local` | 16 | 24 | 24 |

```sh
cargo build --locked --release --bin plenty
target/release/plenty --release --dump ops,clif-opt,asm \
    experiments/kernels/field-rng/kernels.plenty -o /tmp/fr
python3 experiments/kernels/loops.py < /tmp/fr/asm.txt
gcc -O2 -fno-tree-vectorize -c experiments/kernels/field-rng/kernels.c -o /tmp/fr.o
objdump -d --no-show-raw-insn /tmp/fr.o | python3 experiments/kernels/loops.py
```

## Attribution

`fill`, 35 to 29:

| Cause | Before | After |
| --- | --- | --- |
| Tag test of the inlined `Ok` (`xor; test; je`) | 3 | 0 |
| Field read again after its store | 1 | 0 |
| Register moves and the latch `jmp` | 5 | 3 |
| Overflow checks (`jo`; `seto; test; jne`) | 4 | 4 |
| Floor fixup of the remainder (`mov; sar; and; add`) | 4 | 4 |
| Useful work (C: 15) | 18 | 18 |

The method's `Ok(self.seed)` now jumps to the code after `?`
([known returns](../../../book/src/design/toolchain/02-architecture/emission.md#known-returns)),
and the address of `rng` is one value in the function, so Cranelift forwards
the stored seed to the read that follows
([reference parameters](../../../book/src/design/toolchain/02-architecture/emission.md#reference-parameters)).
Two of the removed moves carried the loop's stop across the tag test.

The remaining 8 instructions of checks and fixup depend on the field's range,
which the compiler does not know (issue #937): a seed in `0..IM` cannot
overflow `seed * IA + IC` and has a nonnegative remainder. The fork's
Cranelift later fused `seto; test; jne` into one `jo` (issue #934): `fill`
is 27 and `fill_local` 20. In the 18 instructions of
useful work C needs 15: it folds the load into the multiply and steps a
pointer in place of an index.

`fill_local` is unchanged. Its `s` is a remainder below `IM` on entry and
after every pass, but a local written in the loop carries no bound into the
next pass, so it keeps the same 8 instructions (issue #940).

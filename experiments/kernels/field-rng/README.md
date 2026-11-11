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

## Remainder locals and a bounded field (issues #940 and #937)

Plenty before = `9daa4a78`, same Cranelift fork and gcc. `fill_bounded` is
`fill` with the field typed `Bounded[i64, 0, 139967]` and stored with
`Seed.from(... % IM)`; its C counterpart states the range the only way C can,
with a `uint64_t` field. `fill_arms` writes the local on both arms of an `if`
with different divisors, `fill_inner` writes it only in an inner loop, and
`fill_unbounded` has one write that is not a remainder.

Instructions per pass, and cycles per pass on one pinned core of a Ryzen 7
7840HS (`perf stat -r 5`, three interleaved rounds, 409.6 million passes; the
rounds agree within 0.1%):

| Kernel | C | Before | After | C cycles | Before cycles | After cycles |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `fill` | 15 | 24 | 24 | 20.0 | 18.0 | 18.0 |
| `fill_bounded` | 12 | n/a | 13 | 19.0 | n/a | 13.0 |
| `fill_local` | 16 | 20 | 13 | 14.0 | 18.3 | 13.0 |
| `fill_arms` | 18 and 17 | 39 | 30 | 13.0 | 18.7 | 14.5 |
| `fill_inner` | 15 | 20 | 13 | 14.0 | 18.3 | 13.0 |
| `fill_unbounded` | 18 | 23 | 23 | 16.0 | 19.0 | 19.0 |

`loops.py` counts both arms of `fill_arms` as one loop; a pass runs 16
instructions on the `% IM` arm and 20 on the `% 1000` arm, against C's 18 and
17 (gcc duplicates the latch).

```sh
target/release/plenty --release --compile \
    experiments/kernels/field-rng/kernels.plenty -o /tmp/k
gcc -O2 -fno-tree-vectorize -fno-inline experiments/kernels/field-rng/kernels.c -o /tmp/kc
taskset -c 3 perf stat -r 5 -e instructions:u,cycles:u /tmp/k fill_local 100000
taskset -c 3 perf stat -r 5 -e instructions:u,cycles:u /tmp/kc fill_local 100000
```

### Attribution

`fill_local`, 20 to 13. The local is in `0..IM - 1` on every read
([value bounds](../../../book/src/design/toolchain/02-architecture/emission.md#value-bounds)):

| Cause | Before | After |
| --- | ---: | ---: |
| Overflow checks (`jo`, twice) | 2 | 0 |
| Signed magic division (`sar; mov; shr; add`) against unsigned (`shr`) | 4 | 1 |
| Floor fixup of the remainder (`mov; sar; and; add`) | 4 | 0 |
| Register moves | 1 | 3 |
| Multiply, add, magic multiply, multiply back, subtract, store, step, compare, branch | 9 | 9 |

Two of the three moves are extra. `mov %r8,%rdx` copies the dividend to a
register nothing reads, and `mov %r8,%rax` copies the remainder for the store
although `%r8` holds it; the third loads `%rax` for `mulq`. C's signed
division keeps its sign correction (`mov; sar; sub`), so it runs 16.

`fill_bounded`, 13 against C's 12 with an unsigned field: Plenty loads the
seed with a separate `mov (%rdi),%rax` where gcc folds the load into `imul`.
The loop has no call, no tag test and no stack traffic: `Seed.from` is a
retyping of the remainder, and `get` an ordinary load. C's cycles are higher
on this CPU in both field kernels; its count is still the instruction floor.

`fill_arms`, `% 1000` arm, 20 against C's 17: `imul $3; lea 1(...)` where gcc
uses one `lea 1(%rax,%rax,2)`, the same dead dividend copy as `fill_local`,
and a `jmp` to the shared store, which gcc duplicates. The division itself is
6 instructions in both: Plenty's 65-bit unsigned multiplier
(`mulq; mov; sub; shr; add; shr`) and C's signed sequence with its sign
correction. A dividend known to be below 2^63 needs only `mulq; shr`;
Cranelift does not use the bound to pick that multiplier. The `% IM` arm
runs 16 against C's 18.

`fill_unbounded` is unchanged: `s = s + i` is not a remainder, so `s` keeps
both overflow checks and the floor fixup.

## Checker intervals (issue #952)

Plenty before = `1ade579c`. The checker now proves `seed.get() * IA + IC`
inside `i64`, so `BoundedRng.next` returns a plain `i64` and `fill_bounded`
returns `()`. The generated loops are unchanged, because code generation's
value bounds already removed these checks: every kernel has the same
instructions per pass, and `perf stat -r 5` on one pinned core, three
interleaved rounds (load average 1.6 to 2.0), gives the same cycles within
0.1% (`fill_bounded` 5,330M instructions and 5,330M cycles for 409.6 million
passes, before and after).

`next` returns the local it stored, not `self.seed.get()`: a method whose
final expression is a call is not inlined (issue #958), which costs 9
instructions per pass here.

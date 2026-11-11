# Reads of a `bytes` literal

The repeated-sequence copy of the fasta program in `benchmarks/game/`
(`block.set(i, ALU[position])` with a wrapping `position`) and iteration over
a literal, as standalone functions in `kernels.plenty` and `kernels.c`
(found while working on issues #932 and #933).

Optimized x86-64 Linux builds, Plenty before = `f0a789a7`, Cranelift 0.137.0,
gcc 16.2.1 `-O2 -fno-tree-vectorize`. Counts are instructions on the path one
pass of the loop executes, read from the `asm` dump; `loops.py` also counts
the cold `set` call block that sits inside `fill`'s loop range.

| Kernel | C | Before | After | Issue #939 | regalloc2 change |
| --- | --- | --- | --- | --- | --- |
| `fill` | 8 | 34, four runtime calls | 26 | 17 | 14 |
| `checksum` | 5 | 18, two runtime calls | 6 | 6 | 6 |

The "Issue #939" column is the compiler at `2d20ddf5` with the lowering
change for issue #939; `2d20ddf5` itself still measured 26. The last column
is that compiler built against branch `issue-939-loop-splits` of the
regalloc2 fork (`docs/spill-bundle-registers.md` there) through a path
override; the fork revision in `Cargo.lock` does not have the change yet.

The whole program (100 calls of `fill` over 61,440 bytes, where `fill` is
inlined into `main`) runs 461,119,314 instructions before and 129,290,430
after: 75 and 21 per byte. With the change for issue #939 it runs 117,000,712
(19 per byte), and the fasta benchmark at 2,500,000 runs 1,431,249,475
instead of 1,521,250,112. With the regalloc2 change they run 104,713,167
(17 per byte) and 1,384,582,694.

```sh
cargo build --locked --release --bin plenty
target/release/plenty --release --dump ops,clif-opt,asm \
    experiments/kernels/bytes-literals/kernels.plenty -o /tmp/bl
python3 experiments/kernels/loops.py < /tmp/bl/asm.txt
gcc -O2 -fno-tree-vectorize -c experiments/kernels/bytes-literals/kernels.c -o /tmp/bl.o
objdump -d --no-show-raw-insn /tmp/bl.o | python3 experiments/kernels/loops.py
```

## Attribution

`fill`:

| Cause | Before | After | Issue #939 | regalloc2 change |
| --- | --- | --- | --- | --- |
| Runtime calls (`plenty_byte_str_item`, `plenty_bytes_len`, `plenty_bytes_set`, `plenty_text_byte_len`) | 4 | 0 | 0 | 0 |
| Argument moves, spills and reloads | 15 | 11 | 2 | 0 |
| Overflow check on `position += 1` (`seto; test; jne`) | 3 | 0 | 0 | 0 |
| Bounds checks on the buffer and, after, the literal (C has none) | 4 | 4 | 4 | 4 |
| Trip counter beside the index, unrotated `jmp` | 4 | 1 | 1 | 0 |
| Useful work (C: 8) | 4 | 10 | 10 | 10 |

The useful work grew because the byte load, the store and the address
arithmetic are now in the loop instead of in the runtime.

Before, indexing the literal and reading its length were runtime calls, and
because the frontend did not classify a `bytes` read, the loop kept no cached
length and rows for `block`: `set` was a length call, a compare and a store
call. Now the literal's byte is one load behind `cmp $287; jae`, the wrap
test is `cmp $286; je`, and `set` is one compare with the cached length and a
byte store. `position` passed the literal's index check, so its step is a
plain add; that fact survives the `set` guard, whose failing arm is a cold
call.

The 11 moves were regalloc2's response to the failing arm of the `set`
guard. That arm was the inlined body of `Bytes.set`: a second length call, a
compare, the runtime's store and a trap, in blocks with successors. The
regalloc2 fork keeps a cold block off its loop's allocation only when the
block has no successor, so the cached length was spilled and reloaded on each
pass and values were shuffled between the registers each call wanted. The
failing arm is now one block: the method's arguments are stored to a stack
slot, the caller context is entered, the arguments are loaded and `Bytes.set`
is called, then a trap. 9 of the 11 moves are gone.

The arguments go through the slot because of `read` in
`tests/test_byte_views.rs`. With them in registers they are live across
`plenty_caller_enter`, and that loop went from 7 instructions to 10
(`clif/read.clif`, a against f).

What remained after the lowering change, against C's 8:

| Cause | Instructions |
| --- | --- |
| `mov %r15,%r10` before the `set` guard, `mov %r10,%r15` and `jmp` on the back edge | 3 |
| Bounds checks on the literal and the buffer | 4 |
| `lea` of the literal's byte address, a block parameter shared with the negative-index retry block | 1 |
| Wrap by branch (`cmp; je; add; jmp`) where C has `add; cmp; cmov` | 1 |

The move pair is regalloc2's and no cold block causes it: `clif/fill.clif`
variant e, where both failing arms only trap, keeps it. The cached length
comes from `plenty_bytes_len` before the loop and is live across
`plenty_byte_view_rows`, which clobbers the register it was returned in, so
its bundle is split there. Splitting trims the piece with the loop's use to
that use and gives the rest of the live range, including the loop header
and the back edge, to the value's spill bundle. The piece is allocated
first and takes a caller-saved register; the spill bundle is allocated last
and, crossing the call, takes a callee-saved one. The value then changes
register before its use and changes back on the back edge.

The regalloc2 change moves each such piece into its spill bundle's register
when that register is free for the whole piece. The three instructions go
and `fill` is 14: C's 8, the four of the bounds checks, the `lea` and the
wrap's branch.

In fasta's `random_fasta` the same split leaves two values in stack slots:
each is loaded before its use and stored back unchanged on each pass (two
loads and two stores per symbol). Their spill bundles get no register, because the values are live
across `write_lines` in the enclosing loop and the callee-saved registers
are taken, so the regalloc2 change has nothing to move them into, although
`r10` and `r11` are unused in the per-symbol loop. Issue #939 stays open
for this.

`clif/fill.clif` and `clif/read.clif`, `loops.py` counts (for `fill` they
include one instruction of the wrap's other arm):

| Variant | `fill` | `fill`, regalloc2 change | `read` |
| --- | --- | --- | --- |
| a: cold block calls the method between `caller_enter` and `caller_leave`, arguments in registers | 18 | 15 | 10 |
| b: a without `caller_enter` and `caller_leave` | 18 | 15 | 7 |
| c: cold block only traps | 18 | 15 | 7 |
| d (`fill`): a without the negative-index retry block | 17 | 14 | |
| e (`fill`): c and d | 17 | 14 | |
| f (`fill`), d (`read`): cold block calls a helper with a fresh symbol address | 18 | 15 | 7 |
| e (`read`): method call first, then a helper | | | 7 |
| f (`read`): a with the arguments in a stack slot across `caller_enter`, as emitted | 18 | 15 | 7 |

`read` is unchanged by the regalloc2 change in every variant, including the
10 of variant a.

`checksum`:

| Cause | Before | After |
| --- | --- | --- |
| Runtime calls and their argument moves (`plenty_text_byte_len`, `plenty_byte_str_item`) | 6 | 0 |
| Overflow check on the hidden index (`seto; test; jne`) | 3 | 0 |
| Unrotated loop (`jmp`) | 1 | 0 |
| Register moves | 2 | 0 |
| Overflow check on the `u64` sum (`jb`; can fail) | 1 | 1 |
| Useful work (also in C) | 5 | 5 |

The length is read once before the loop, and is the constant 287 here, so
the hidden index is a counter below a constant: no check, and the loop ends
in `add; cmp $287; jl`.

# Reads of a `bytes` literal

The repeated-sequence copy of the fasta program in `benchmarks/game/`
(`block.set(i, ALU[position])` with a wrapping `position`) and iteration over
a literal, as standalone functions in `kernels.plenty` and `kernels.c`
(found while working on issues #932 and #933).

Optimized x86-64 Linux builds, Plenty before = `f0a789a7`, Cranelift 0.137.0,
gcc 16.2.1 `-O2 -fno-tree-vectorize`. Counts are instructions on the path one
pass of the loop executes, read from the `asm` dump; `loops.py` also counts
the cold `set` call block that sits inside `fill`'s loop range.

| Kernel | C | Before | After |
| --- | --- | --- | --- |
| `fill` | 8 | 34, four runtime calls | 26 |
| `checksum` | 5 | 18, two runtime calls | 6 |

The whole program (100 calls of `fill` over 61,440 bytes, where `fill` is
inlined into `main`) runs 461,119,314 instructions before and 129,290,430
after: 75 and 21 per byte.

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

| Cause | Before | After |
| --- | --- | --- |
| Runtime calls (`plenty_byte_str_item`, `plenty_bytes_len`, `plenty_bytes_set`, `plenty_text_byte_len`) | 4 | 0 |
| Argument moves, spills and reloads | 15 | 11 |
| Overflow check on `position += 1` (`seto; test; jne`) | 3 | 0 |
| Bounds checks on the buffer and, after, the literal (C has none) | 4 | 4 |
| Trip counter beside the index, unrotated `jmp` | 4 | 1 |
| Useful work (C: 8) | 4 | 10 |

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

The 11 moves are regalloc2's: the loop holds two call blocks (the trapping
`set` and its length query), so the cached length is spilled and reloaded on
each pass and values are shuffled between the registers each block wants.
Not patched here (issue #939). C uses `cmov` for the
wrap; Plenty branches (`cmp; je; add; jmp`).

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

# fannkuch-redux kernel probes

Evidence for local PR #917: why `examples/fannkuch_redux.plenty` runs more
instructions than C, loop by loop, and which IR shapes change that. x86-64
Linux; needs gcc, GNU objdump, perf and Rust.

| File | Contents |
|---|---|
| `fannkuch.c` | The benchmark in C, with the same workers, blocks and output |
| `kernels.plenty`, `kernels.c` | The benchmark's hot loops as standalone functions; `reverse_swap` has no C counterpart, compare it with `reverse` |
| `loops.py` | Counts the instructions one pass of each innermost loop runs |
| `harness/` | Compiles hand-written CLIF with Plenty's release flags and disassembles it |
| `clif/reverse.clif` | The reversal loop in variants a–m, one IR shape changed in each |
| `clif/cold_args.clif` | Variants n–y: what a bounds check's cold block can pass to its helper |

Count instructions with `perf stat -e instructions:u`, pinned with
`taskset -c 0-7`; wall-clock times on this laptop are noise.

From the repository root, after `cargo build --release --locked --bin plenty`:

```sh
gcc -O2 -fno-tree-loop-distribute-patterns -c experiments/kernels/kernels.c -o /tmp/kc.o
objdump -d --no-show-raw-insn /tmp/kc.o | python3 experiments/kernels/loops.py

target/release/plenty --dump asm experiments/kernels/kernels.plenty -o /tmp/k
# Lowercase, unpad hex (0x0000001d -> 0x1d), and turn `==== name` into `==== x name`:
python3 -c 'import re,sys
for l in sys.stdin:
    l=re.sub(r"0x0*([0-9A-Fa-f]+)",lambda m:"0x"+(m.group(1).lower() or "0"),l)
    print(re.sub(r"^==== (\S+)",r"==== x \1",l) if l.startswith("====") else l.lower(),end="")' \
  < /tmp/k/asm.txt | python3 experiments/kernels/loops.py

cargo build --release --manifest-path experiments/kernels/harness/Cargo.toml \
  --target-dir target/kernel-harness
target/kernel-harness/release/plenty-kernel-harness experiments/kernels/clif/reverse.clif \
  | python3 experiments/kernels/loops.py
```

gcc 16.2.1 gives the same counts at `-O1` and `-O2`. At `-O2`, gcc turns
`rotate_left` into `memmove` unless `-fno-tree-loop-distribute-patterns` is
given.

## Results

The tables below record the named historical revisions. The current kernels
propagate `OverflowError[i64]` through `Result`, matching the arithmetic
contracts in `examples/fannkuch_redux.plenty`. Re-run the dump commands to
compare current code; these tables are not measurements of the migrated ABI.

Instructions per pass:

| Kernel | C | Plenty at ec49764a | 4d4f1545 (index operands in memory) | 25a374cc (range facts) |
|---|---|---|---|---|
| reverse | 8 | 22 | 14 | 14 |
| rotate_left | 5 | 23 | 18 | 13 |
| copy_row | 5 | 19 | 15 | 11 |
| count_flips, inner | 8 | 31 | 14 | 14 |
| count_flips, outer path | 10 | 29 | 21 | 21 |

Since issue #729, `count_flips` and `flips` reverse with `s.swap(lo, hi)`
through the `&mut [i64]` view. `reverse_swap` is `reverse` written that way;
both run 14 instructions per pass, as does `count_flips`' inner loop, so the
C comparison is unchanged. The two forms differ only in register choice and
one fewer trap site.

Issue #726 rotates loops into do-while form, so each pass ends in one
conditional branch. Measured on the current kernels, per pass, before and
after:

| Loop | 4239252a | Rotated |
|---|---|---|
| reverse | 14 | 13 |
| reverse_swap | 9 | 8 (C: 8) |
| rotate_left, checked copy | 12 | 12 |
| rotate_left, unchecked copy | 8 | 7 |
| copy_row, checked / unchecked | 9 / 7 | 8 / 6 |
| count_flips, checked / unchecked | 9 / 9 | 8 / 8 |

`rotate_left`'s checked copy keeps 12: regalloc2 splits the latch's back
edge and places a `mov` and `jmp` there.

`reverse.clif` (a is Plenty's shape before 4d4f1545):

| Variant | Change | Count |
|---|---|---|
| a | as dumped | 22 |
| b | `hi` known nonnegative | 22 |
| c | slice split once, before the loop | 22 |
| d | c, rotated | 20 |
| e | d with `trapz` checks, no helper call | 12 |
| f | d with one check hoisted before the loop | 8 |
| g | b's checks, c's split, unrotated | 9 |
| h | d with one shared cold block taking the index | 17 |
| i | d with the index as each cold block's parameter | 20 |
| j | c with i's parameters | 22 |
| k | d with index and length stored to a stack slot, address passed | 15 |
| l | k, unrotated: Plenty's shape since 4d4f1545 | 15 |
| m | k with the site as first argument | 15 |

`cold_args.clif`, each based on d:

| Variant | Cold block | Count |
|---|---|---|
| n | call with a fresh symbol address only | 12 |
| o | trap only | 12 |
| p | store index to a stack slot, trap | 12 |
| q | store index and length, trap | 12 |
| r | call with the length | 19 |
| s | call with the index | 20 |
| t | call with the index, in the first cold block only | 15 |
| u | store to a global record, call with the site | 15 |
| v | store to a stack slot, call with the site | 12 |
| w | store to a stack slot, call with its address and the site | 15 |
| x | w with a `stack_addr` per cold block | 15 |
| y | store to a global record, trap | 15 |

Any hot value passed to the cold call costs 3 to 8 instructions per pass:
regalloc2 places the loop's live ranges in the call's fixed argument
registers instead of splitting them at the cold block's entry. Passing only
the record's address (w) costs 3; Cranelift GVN merges each block's
`stack_addr` and hoists it to a dominator (x), so a separate slot per check
does not help.

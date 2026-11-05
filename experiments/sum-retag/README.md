# std.bytes.text_in probes

Evidence for local PR #953 (issue #757): why `std.bytes.text_in` ran 57
instructions of its own per call, and which IR shapes change that. x86-64
Linux; needs gcc, GNU objdump, valgrind and Rust.

| File | Contents |
|---|---|
| `kernel.plenty` | A `to_text_range` loop over 4-byte windows: 4,093 `text_in` calls on the `Ok(Some)` path |
| `text_in.c`, `driver.c` | `text_in` in C against the same runtime call, and a loop that runs it 4,093 times |
| `paths.py` | Lists every path through a loop-free function, with its instruction count and returned tag |
| `clif/a.clif` … `clif/g.clif` | `text_in`'s optimized CLIF, one fix added per variant |

The CLIF files are `text_in` from `--dump clif-opt` of the kernel at each
step of the change (a–f) and one hand-written shape (g). They compile with
the harness in `experiments/kernels/harness`.

From the repository root, after `cargo build --release --locked --bin plenty`:

```sh
mkdir -p target/sum-retag
gcc -O2 -c experiments/sum-retag/text_in.c -o target/sum-retag/text_in.o
gcc -O2 experiments/sum-retag/driver.c target/sum-retag/text_in.o -o target/sum-retag/c
objdump -d --no-show-raw-insn target/sum-retag/text_in.o | python3 experiments/sum-retag/paths.py rdx
valgrind --tool=callgrind --callgrind-out-file=target/sum-retag/c.out target/sum-retag/c

PLENTY_CACHE=off target/release/plenty --compile experiments/sum-retag/kernel.plenty \
  --no-strip -o target/sum-retag/plenty
valgrind --tool=callgrind --callgrind-out-file=target/sum-retag/plenty.out target/sum-retag/plenty
callgrind_annotate target/sum-retag/plenty.out | grep text_in

cargo build --release --manifest-path experiments/kernels/harness/Cargo.toml \
  --target-dir target/kernel-harness
for v in a b c d e f g; do
  target/kernel-harness/release/plenty-kernel-harness experiments/sum-retag/clif/$v.clif \
    | python3 experiments/sum-retag/paths.py
done
```

Callgrind's self cost of `text_in` divided by its call count (4,093 in C;
4,094 in Plenty, where `to_text` adds one call so `text_in` is not inlined)
gives the instructions per call. The hot path in `paths.py` output is the
shortest whose high word is cleared (`xorq %rcx, %rcx`: `Ok(text)`); in
variant a it is the one that copies `%r14`.

## Results

Instructions per call on the `Ok(Some)` path, gcc 16.2.1:

| Variant | Change | Hot path | Total |
|---|---|---|---|
| C | `gcc -O2` | 24 | 31 |
| a | Before the change (bc676707); callgrind 57.0 per call | 57 | 100 |
| b | The `Nothing` arm writes the shared empty slot to the local, so its release emits nothing | 41 | 70 |
| c | One-word scratch operands are stored as the word and a 64-bit zero | 34 | 61 |
| d | Re-tagging a value whose tag the arm knows reuses the matched value | 32 | 55 |
| e | `split` and `raw_word` see through `iconcat` and `uextend` | 30 | 48 |
| f | Matches on one-bit tags branch on the bit; callgrind 28.0 per call | 28 | 38 |
| g | f with the join after the inlined match threaded by hand | 26 | 38 |

b removes the release of the matched `Option[str]` and with it 5
callee-saved register saves and 5 restores. c leaves no callee-saved
registers. Of the 4 over C, g shows that 2 are the `?` re-testing the tag
after the inlined match's join; Cranelift 0.137 does not thread that join,
whose block parameter is used after it. The frame pointer push, move and
pop add 3 that C does not have.

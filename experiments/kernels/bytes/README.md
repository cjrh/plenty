# Bytes method access kernels

Issue #741: isolate `Bytes.get`/`set` through an exclusive buffer parameter.
`kernels.plenty` and `kernels.c` transform the same four bytes. The C loop
has the same byte element and signed length types; disable vectorization to
compare scalar passes separately from vector width.

Build the Plenty compiler in this worktree with
`cargo build --locked --release --bin plenty`, then:

```sh
PLENTY_CACHE=off target/release/plenty --dump ops,clif,clif-opt,asm \
    --dump-filter transform experiments/kernels/bytes/kernels.plenty -o /tmp/byte-kernel

gcc -O2 -fno-tree-loop-distribute-patterns -fno-tree-vectorize \
    -fno-unroll-loops -fno-inline experiments/kernels/bytes/kernels.c -o /tmp/byte-kernel-c
objdump -d --no-show-raw-insn /tmp/byte-kernel-c | python3 experiments/kernels/loops.py
```

Normalize the Plenty assembly for `loops.py` using the command in the
[parent README](../README.md). Its count includes the caller's instructions
and calls, but excludes the instructions executed inside native helpers.
Use `perf stat -e instructions:u` on the resulting executables and the
reverse-complement benchmark for end-to-end counts.

The frozen `dispatch.clif` is the optimized kernel before this change;
`registers.clif` changes only the two byte accesses from the descriptor and
scratch-block ABI to register arguments. Compile both with the existing
[CLIF harness](../harness/):

```sh
target/kernel-harness/release/plenty-kernel-harness \
    experiments/kernels/bytes/dispatch.clif experiments/kernels/bytes/registers.clif \
    | python3 experiments/kernels/loops.py
```

These CLIF files are assembly experiments, not linkable programs: the
external function numbers describe the ABI shape rather than exported
symbols. The variant comparison establishes whether bypassing the dispatcher
can improve the hot path before changing native lowering.

## Recorded result

Measured on 2026-11-07, baseline `2ba554c3`, AMD Ryzen 7 7840HS, x86-64
Linux, rustc 1.99.1, pinned Cranelift 0.137.0 and gcc 16.2.1. Both compilers
were built in the task worktree with `cargo build --locked --release`.
The baseline dump differs from `dispatch.clif` only in source display metadata.

| Build / variant | Caller instructions per byte |
|---|---:|
| Scalar C, flags above | 4 |
| Plenty baseline | 62 |
| Frozen CLIF, replace only dispatcher argument/result ABI | 38 |
| Plenty narrow byte helpers | 38 |

Every hot-path instruction belongs to one row below. Counts exclude native
callee bodies, which the whole-program measurement includes.

| Cause | Before | After | Explanation |
|---|---:|---:|---|
| Dispatcher scratch arguments/results and associated moves | 24 | 0 | Replaced by register arguments and direct scalar results |
| Caller trap-site scopes | 10 | 10 | Site address, enter, save, reload and leave for each method |
| Strict index checks and repeated length queries | 12 | 12 | Signed test/branch, owner load, length call, comparison/branch per method |
| Unused high-word owner loads | 2 | 2 | `ReadRef` loads both value words; the high word is unused |
| Native access, arguments, result and XOR | 10 | 10 | Get: owner/index moves, call, result move; set: owner/result/index moves, XOR, byte extension, call |
| Loop control | 4 | 4 | Down-counter decrement, index increment, test and branch |
| Total | 62 | 38 | C combines its load, XOR and store into one memory instruction, plus three loop instructions |

The kernel prints `b"acgt"` in both languages. The benchmark runner verifies
reverse-complement's check input and quick input against C's full output.
Three additional executable runs, pinned to physical core 0, give:

| Workload | Before instructions | After instructions |
|---|---:|---:|
| Reverse-complement, quick FASTA size 2,500,000 | 5,537,861,035–5,537,861,161 | 4,134,110,350–4,134,110,522 |
| `compile_bench 1000 10 --workload aot` | 122,394,668,272–122,418,632,728 | 122,391,590,164–122,417,389,282 |
| Compile reverse-complement | 168,628,519–171,734,437 | 163,561,448–168,689,317 |
| Compile `examples/fannkuch_redux.plenty` | 352,159,565–357,241,000 | 352,063,024–357,222,715 |

Reverse-complement executes **25.3% fewer instructions**. C's quick input uses
362,148,217–362,148,280 instructions: the Plenty/C instruction ratio falls
from 15.29 to 11.42. The compile probe is unchanged within its spread; the
single-program compile counts include linker variation and do not establish a
compiler-speed improvement. Wall-clock measurements are deliberately omitted
because other agents were building on the same machine. Raw repetitions are
in `results.json`.

Reproduce the whole-program check/count with the before and after release
compiler paths, preserving each result before the runner rebuilds its binaries:

```sh
PLENTY_CACHE=off python3 -B benchmarks/game/bench.py --plenty target/release/plenty \
    --work /tmp/byte-game run reverse-complement --mode quick --repeats 3 \
    --perf --output /tmp/byte-game-results.json
for run in 1 2 3; do
    perf stat -e instructions:u taskset -c 0 \
        /tmp/byte-game/bin/reverse-complement.plenty-idiomatic 0 \
        < /tmp/byte-game/inputs/fasta-2500000.txt > /dev/null
done
PLENTY_CACHE=off perf stat -e instructions:u target/release/examples/compile_bench \
    1000 10 --workload aot
```

Repeat the compile probe and each `plenty --compile` command three times with
`PLENTY_CACHE=off`, counting `instructions:u`. These compile counts include the
probe's warm-up, its ten measured iterations, and linker subprocesses.

Remaining measured costs were local issues #842 (trapping std caller scopes),
#843 (repeated length/bounds checks and native byte access), and #844 (unused
high-word loads). Removing guards or scopes requires new proofs and diagnostic
coverage; this change removes neither. No backend dependency patch is needed
for the ABI improvement demonstrated here.

## Loop-cached access (issue #843)

Measured on 2026-11-07, baseline `4cfb5440`, same machine and toolchain. A
loop that cannot resize or replace the buffer reads its length (here the
range's stop) and first-byte address once; `get` and `set` lower to
`BytesGetView`/`BytesSetView`. The range proves `i` in bounds, so each access
is one byte load or store.

| Build | Caller instructions per byte |
|---|---:|
| Scalar C, flags above | 4 |
| Plenty before | 38 |
| Plenty after | 7 |

| Cause | Before | After | Explanation |
|---|---:|---:|---|
| Caller trap-site scopes (#842) | 10 | 0 | The method calls leave the hot loop; the scope around the hoisted `len()` runs once |
| Strict index checks and repeated length queries | 12 | 0 | The range bounds `i` by the cached length, which no operation in the loop can change |
| Unused high-word owner loads (#844) | 2 | 0 | The owner is read once, before the loop |
| Native access, arguments, result and XOR | 10 | 3 | `movzbq` load, XOR and byte store; C folds them into one `xorb` to memory |
| Loop control | 4 | 4 | Down-counter decrement, index increment, test and branch; C uses three |
| Total | 38 | 7 | |

An index the loop cannot prove in range, such as `range(start, stop)` with
parameter bounds, adds one unsigned comparison and branch per access (10
instructions per byte for a single `get` loop). A failing index calls the
method from a cold block, so messages, std positions and caller notes are
unchanged. Loops that may resize the buffer, through `push` or a call taking
`&mut Bytes`, keep the method calls and their #842/#844 costs.

| Workload | Before instructions | After instructions |
|---|---:|---:|
| Reverse-complement, quick FASTA size 2,500,000 | 4,133,999,769–4,133,999,784 | 765,707,632–765,707,639 |
| `compile_bench 1000 10 --workload aot` | 125,050,332,769–125,070,177,029 | 125,000,957,090–125,037,322,651 |
| Compile reverse-complement | 163,152,628–168,593,317 | 165,684,193–169,826,176 |
| Compile `examples/fannkuch_redux.plenty` | 352,970,411–367,450,272 | 353,865,405–358,970,620 |

Reverse-complement executes **81.5% fewer instructions** with identical output.
C's quick input uses 362,036,283–362,036,296 instructions: the Plenty/C ratio
falls from 11.42 to 2.11. Compile counts are unchanged within their spread.
The before compile probe was built from the baseline source in a separate
target directory. Raw repetitions are under `issue-843` in `results.json`.

## Caller sites in inlined access (issue #842)

`growing.plenty` pushes to the buffer it reads, so the loop keeps the method
calls. Before this change, each inlined `len`, `get`, `set` and `push` saved
and restored the thread-local caller: a site address, `plenty_caller_enter`,
a spill and reload of its result and `plenty_caller_leave`. Inlined `Bytes`
primitives now name the user call site in their own trap records instead.

```sh
PLENTY_CACHE=off target/release/plenty --release --dump asm \
    --dump-filter transform experiments/kernels/bytes/growing.plenty -o /tmp/growing
```

| Kernel | Instructions per byte, 2e50b1b8 | After | `plenty_caller_enter` calls in `transform`, before / after |
|---|---:|---:|---|
| `growing.plenty` | 68 | 45 | 5 / 0 |
| `growing.plenty`, pushing to a second `&mut Bytes` | 68 | 45 | 5 / 0 |
| `kernels.plenty` | 7 | 7 | 1 / 0 (around the hoisted `len()`) |

The reverse-complement benchmark's reversal loop is a `while` loop, so it
still calls `get` and `set`; it fell from 53 to 45 instructions per pass, and
the program's `plenty_caller_enter` call sites from 32 to 14. The
quick FASTA input (size 2,500,000) ran 765,708,420–765,708,439
`instructions:u` before and 656,456,084–656,456,096 after, **14.3% fewer**,
with identical output (three runs each, `taskset -c 0`). The before compiler
was built from 2e50b1b8 in a separate target directory.

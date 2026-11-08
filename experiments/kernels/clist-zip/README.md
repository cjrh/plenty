# Selected-field column zip — issue #888

Independent reproduction and fix based on local main `ad134bd6`, rather than
an extension of the unmerged tutorial PR #1113. x86-64 Linux, Plenty 0.8.19,
Rust 1.99.1, gcc 16.2.1, Cranelift/regalloc2 0.137.0/0.15.2, AMD Ryzen 7
7840HS with scheduler-default affinity. All Plenty program
measurements use optimized compilation; no allocation instrumentation.

`scan.plenty` compares explicit zip, zip/fold and indexed selection with a
runtime threshold. `scan.c` is the equivalent scalar C kernel. The full Plenty
program builds 8,192 rows and repeats all three scans 200 times, returning the
checksum `2808000.0`. This includes real inlined callers, not only standalone
functions retained by dumps. The input, comparisons, floating types and
nonnegative lengths agree. No SIMD or unchecked source arithmetic is used. `results.json` retains the
counts and SHA-256 hashes of the sources, compiler binaries and executables.

## Reproduction

Build each compiler revision in its own worktree and save its executable before
changing the source. From this worktree, with `BEFORE` naming the baseline
compiler and `AFTER` the changed one:

```sh
cargo build --locked --release --bin plenty
PLENTY_CACHE=off "$BEFORE" --dump ops,clif-opt,asm experiments/kernels/clist-zip/scan.plenty -o /tmp/zip-before
PLENTY_CACHE=off "$AFTER" --dump ops,clif-opt,asm experiments/kernels/clist-zip/scan.plenty -o /tmp/zip-after
python3 experiments/kernels/loops.py < /tmp/zip-before/asm.txt
python3 experiments/kernels/loops.py < /tmp/zip-after/asm.txt

gcc -O2 -fno-tree-vectorize -fno-tree-loop-distribute-patterns -c experiments/kernels/clist-zip/scan.c -o /tmp/zip-c.o
objdump -d --no-show-raw-insn /tmp/zip-c.o | python3 experiments/kernels/loops.py

cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml --target-dir target/kernel-harness
target/kernel-harness/release/plenty-kernel-harness experiments/kernels/clist-zip/variants.clif > /tmp/zip-variants.txt
python3 experiments/kernels/loops.py < /tmp/zip-variants.txt

PLENTY_CACHE=off "$BEFORE" --compile experiments/kernels/clist-zip/scan.plenty -o /tmp/zip-before-program
PLENTY_CACHE=off "$AFTER" --compile experiments/kernels/clist-zip/scan.plenty -o /tmp/zip-after-program
perf stat -r 3 -x, -e instructions:u /tmp/zip-before-program
perf stat -r 3 -x, -e instructions:u /tmp/zip-after-program
```

The harness lock now aligns its five Cranelift assembler/meta/source-generator
transitive packages with the compiler's 0.137.0 lock. It previously combined
0.137.0 codegen with 0.137.1 transitive packages. The compiler dependency pins
are unchanged. Both harness versions gave the same results below; the final
comparison used the aligned lock.

## Isolated IR experiments before compiler changes

`variants.clif` retains the baseline optimized explicit-loop IR and two hand
variants, with the same column-header preparation and useful computation.
These were compiled and counted before editing compiler lowering.

| Variant | Change | Static innermost-loop instruction span |
| --- | --- | ---: |
| `a_pulls` | Baseline nested pull loops and pending `Option`s | 43 |
| `b_direct` | Direct scalar loads; separate counters and end checks | 12 |
| `c_shared` | One counter; minimum length prepared before the loop | 9 |

The final frontend operations expose a bottom-tested loop that the compiler
rotates, reducing explicit zip to 8. The hand variant keeps its header test
and back-edge jump, accounting for its extra instruction.

## Attribution and result

`loops.py` counts the static instruction span between a detected innermost
back edge and its target. **It is not a dynamic matching-path count:** the
baseline span includes untaken exhaustion blocks. On a successful row that
passes the threshold, manually tracing the baseline explicit zip executes
31 instructions, versus 8 after the change.

The baseline explicit zip's 31 instructions comprise:

| Work on a successful matching row | Instructions |
| --- | ---: |
| Two source end comparisons and branches | 4 |
| Two scalar loads | 2 |
| Pending-item ready/tag flags and tests, option transport/control | 17 |
| Float comparison and branch | 2 |
| Floating addition | 1 |
| Threshold register moves around the consumer | 2 |
| Two counter increments and back edge | 3 |

The flags/control category includes the empty-ready resets, option tags,
branches and transfers into/out of optional integer payloads. No bounds-check
or overflow work is hidden in these hot loops. Owner/header reads occur before
them. The shared-cursor lowering removes the pending options and duplicate
state rather than relying on the backend to reconstruct their relationship.

| Kernel | Before static span | After static span | Scalar C |
| --- | ---: | ---: | ---: |
| Explicit zip | 43 | 8 | 7 |
| Zip/fold with captured threshold | 43 | 10 | 7 |
| Indexed fields | 8 | 8 | 7 |

The actual inlined loops in `main` also have 8, 10 and 8 instructions after the
change. Explicit zip now runs two loads, comparison, conditional branch,
addition, increment, end comparison and conditional back edge. C folds the
weight load into `addsd` and loads it only on matching rows. Indexed Plenty
also folds that load but keeps a separate range countdown, leaving 8.

Fold's two additional instructions reload the immutable closure address and
threshold per row. That remaining concern is local issue #891. It is separate
from zip state, and is also present in actual inlined callers.
These are measurements of the column-zip revision; the subsequent
[conditional-capture experiment](../scalar-captures/README.md) records removal
of those two reloads, bringing fold to 8 instructions too.

Whole-program `instructions:u`, arithmetic mean of three runs:

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `scan.plenty` executable | 118,124,312 | 44,158,708 | -62.62% |

Both print `2808000.0`. The report includes setup, allocation, all three scans,
printing and cleanup. This is an instruction-count result, not an elapsed-time
speedup or a general list-versus-clist comparison. Concurrent builds make
elapsed measurements unsuitable here.

## Compiler cost

For a comparable before/after AOT workload, export the same 1,000-unit program
as `examples/compile_bench.rs`, and count ten separate optimized CLI builds:

```sh
python3 experiments/kernels/clist-zip/compile_workload.py > /tmp/zip-compile.plenty
perf stat -r 10 -x, -e instructions:u env PLENTY_CACHE=off "$BEFORE" --compile /tmp/zip-compile.plenty -o /tmp/zip-compile-before
perf stat -r 10 -x, -e instructions:u env PLENTY_CACHE=off "$AFTER" --compile /tmp/zip-compile.plenty -o /tmp/zip-compile-after
cargo build --locked --release --example compile_bench
perf stat -r 3 -x, -e instructions:u env PLENTY_CACHE=off target/release/examples/compile_bench 1000 10 --workload aot --no-warmup
```

CLI means were 11,643,972,034 before and 11,646,251,790 after (+0.020%), with
0.01%/0.02% reported variation. This does not establish a compilation-cost
regression. The changed library benchmark runner additionally averaged
115,059,439,912 instructions for its ten AOT compilations (three runs). It
avoids repeated CLI startup, so compare the two CLI figures to each other,
not with the library-runner total. This general workload contains no clist zip.

## Legality and limits

The frontend recognizes only the exact unadapted scalar column cursor shape.
It prepares each pointer at its current cursor, computes each remaining length,
and walks the shorter range with one new cursor. Source cursors remain within
`0..=length`; subtracting them cannot overflow, and pointer offsets are ordinary
wrapping address arithmetic. No address is dereferenced on an empty range.
The original owner loans remain active. Construction order, allocation and
scalar load types are preserved.

Moving an owned column plan into zip makes its old cursor unobservable, even
when `next` or a borrowing helper previously advanced it. A zip binding itself
can still be advanced, consumed or lent normally. Plans currently lent from
another function retain the general path because their state must be written
back. Filter/map/take adapters before zip, generators, nested pairs and other
sources also keep left-first pulls and shortest-input semantics. This fix does
not claim that every pipeline or nested multi-column zip has C-like code.

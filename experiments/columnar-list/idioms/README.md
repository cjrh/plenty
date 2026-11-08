# Selected-field tutorial idioms

Issue #887: paired list/clist aggregation, conditional aggregation and simulation
updates from `book/src/tutorial/special-purpose/columnar-lists/index.md`.
`run.py` extracts the actual function bodies from that page. It generates only
the schema extension, input construction, repetitions, timing and checksums.
It does not replace checked source with `unchecked` operations. No compiler or
public API changes are included in this work.

## Reproduce

Build a compiler from this worktree, without allocation instrumentation:

```sh
cargo build --locked --release --bin plenty
python3 -B experiments/columnar-list/idioms/run.py target/release/plenty \
  --out /tmp/clist-idioms --rows 8192 262144 --widths 8 64 \
  --repeats 50 --samples 7 --cpu 8 --require-idle
```

`--require-idle` needs visibility of host processes. It waits for builds and
integration tests and rejects executions overlapping detected processes,
sampling every 250 ms. It does not exclude other operating-system activity.
All Plenty compilation finishes before measurement. Executions are pinned to
logical CPU 8; choose an available CPU on another machine. One warmup and seven
samples alternate list/clist order within each task. Each sample repeats its
pass 50 times. All owners remain allocated during all samples.

Mass values are `(i % 1024) + 2`, temperatures are `i % 40`, and the threshold is
20. Positions start at `i % 1024` and that value plus one; velocities are 0.5
and 1.0 and the measured time step is 0.25. The mutation samples advance both
collections equally; the oracle includes every previous pass. Every checksum
is compared with an independently calculated Python result. Initialization,
printing and the update checksum scan are outside the timed interval. Fold
results contribute to a checksum inside that interval.

The base schema has six `f64` and two `i32` fields. The wide schema appends 56
unused `f64` fields. Both layouts receive identical values. These are repeated
hot scans, not construction, random access, full-object or cold-cache results.
The comparison includes both native packing and columnar layout: Plenty's list
rows use tagged slots, unlike C's native structs. It does not isolate SoA from
representation size. Fixed setup costs can dominate very short passes.

When run, the output `measurements.json` includes all samples (including warmups),
compiler/version/source/binary hashes, the base commit, CPU model, and
whole-process retired instructions. The latter include both layouts, all tasks,
construction, checksums and teardown; they are not per-task instruction counts.
The summary reports the median of paired list/clist time ratios and their full
range. A ratio below one means clist was slower.

For a quick independent correctness check on short inputs and uneven lengths:

```sh
python3 -B experiments/columnar-list/idioms/run.py target/release/plenty \
  --out /tmp/clist-idioms-check --rows 1 7 8 17 129 --widths 8 64 \
  --repeats 2 --samples 2 --verify-only
```

## Retained validation and timing limit

`validation.json` retains the independent checksum checks for both schemas at
1, 7, 8, 17, 129, 8,192 and 262,144 rows, with compiler and source hashes.
`instructions.txt` retains the emitted loop counts. The full tutorial tests
also execute the small example.

Concurrent builds and integration tests prevented a build/test-free window
during this session. No elapsed-time speedup table is retained or claimed.
Exploratory timed samples are excluded; instruction counts remain useful on a
shared machine but cannot measure memory bandwidth or cache misses. The runner
is supplied to obtain a qualified timing comparison later.

## Emitted-code comparison

Use a generated eight-field program from the measurement directory:

```sh
target/release/plenty --dump asm,clif-opt /tmp/clist-idioms/w8-n8192/main.plenty \
  -o /tmp/clist-idioms-dump
python3 experiments/kernels/loops.py < /tmp/clist-idioms-dump/asm.txt
gcc -O2 -fno-tree-vectorize -fno-tree-loop-distribute-patterns \
  -fno-unroll-loops -fno-peel-loops \
  -c experiments/columnar-list/idioms/kernels.c -o /tmp/clist-idioms-c.o
objdump -d --no-show-raw-insn /tmp/clist-idioms-c.o \
  | python3 experiments/kernels/loops.py
```

Measured at compiler source commit `365a3b3b`, Plenty 0.8.19, gcc 16.2.1,
x86-64. Instructions per standalone inner-loop pass, taking the matching branch
for conditional aggregation:

| Task | C AoS | Plenty list | C SoA | Plenty clist |
| --- | ---: | ---: | ---: | ---: |
| Aggregate mass | 4 | 6 | 4 | 5 |
| Conditional mass | 7 | 9 | 7 | 8 |
| Update positions | 11 | 15 | 11 | 14 |

The standalone list aggregate adds a row-stride multiply and a separate scalar
load (C combines its load and add). The clist aggregate adds only that separate
load. Conditional list aggregation adds a multiply and row-address `lea`;
conditional clist aggregation adds a separate countdown update. Both update
loops copy `dt` into a temporary register twice per pass; the list additionally
computes the row address with multiply/`lea`, while the clist additionally
updates its countdown. These account for all extra instructions in this table.
The selected clist loops have no per-row schema dispatch, row reconstruction,
owner reloads, or bounds traps; their range establishes valid indices.

Inspect the `measure_*` callers too: call-site inlining changes register
allocation and loop-counter layout. Counts above are evidence about the
standalone functions, not claims about every inlined caller. For example,
`measure_clist_advance` retains one extra register move in its inner loop.
These C kernels establish a scalar code-shape reference; C's struct layout is
not Plenty's list representation, and C is not part of the timing comparison.
No compiler transformation or IR variant is proposed, so compiler before/after
and compilation-speed comparisons do not apply.

## Zipped-column performance gap (issue #888)

`zip-probe.plenty` retains the supported alternatives considered during tutorial
design: a two-column `zip` followed by `fold`, an explicit zipped loop, and a
loop over indices. All print 7.0 for the same input. To inspect them:

```sh
target/release/plenty --dump asm,clif-opt \
  experiments/columnar-list/idioms/zip-probe.plenty -o /tmp/clist-zip-probe
python3 experiments/kernels/loops.py < /tmp/clist-zip-probe/asm.txt
```

Both zipped forms have 43 instructions on the standalone matching path versus
eight for the indexed loop (seven in the C SoA reference). The IR retains two
source counters and per-source optional-item state; the assembly constructs and
tests presence flags, moves float values through integer registers, and joins
multiple branches before the useful comparison/addition. An explicit zip loop
does not eliminate this state. Inlined measurement callers can be more complex.
This is independent of SIMD and is not necessary to the storage layout.

The tutorial therefore recommends indexed access for multi-field passes today.
Issue #888 records the compiler work; this PR does not fix that lowering.
Initial exploratory timings overlapping a build are excluded from retained
results. The final results use only the recommended indexed idiom.

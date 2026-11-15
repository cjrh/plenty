# Bounded spectral-norm adoption (issue #972)

The spectral-norm benchmark uses `Bounded[i64, 0, MAX_N]` parameters for
`eval_a`, `Index.from` for proven loop indexes, and local length guards for
those proofs. Its arithmetic no longer needs `unchecked:`. Command-line input
validation, allocation, loop/reduction order, errors and printed output stay
the same. The helper's largest accepted arguments are also safe, even though
the benchmark's loop indexes are strictly below `MAX_N`.

The production and CLIF harness lockfiles select the published Cranelift
commit `d27f16528ab79ae70b6e0f4caca1d5e3a787e92b`. Only that scheduling change
is adopted. The separate issue #980 load-folding/loop-block experiment is not
included. Its remaining load is visible in both bounded inner loops.

## Attribution and per-loop gate

Both release compilers use Plenty source `58e5a09b`; their backend revisions
are `98c0373058` (production baseline) and `d27f16528a` (candidate). The original
benchmark is preserved from the same base revision. All function-cache reads
are disabled. Counts come from optimized `--dump asm` and `../../loops.py`,
with the loop instruction lists retained in `loop-counts.json`.

| CPU | Previous backend, original A/AT | Published backend, original A/AT | Published backend, bounded A/AT |
| --- | ---: | ---: | ---: |
| x86-64 | 16/18 | 16/16 | 15/15 |
| x86-64-v3 | 15/17 | 15/15 | 14/14 |

Scheduling removes the extra counter move and split-backedge jump, as
qualified in [counted latch scheduling](../../counted-latch/README.md). The
bounded arguments then prove the multiply cannot overflow, removing its `jo`.
No arithmetic check is suppressed without a proof; the rest of each formula
and the floating reduction retain their order. Register arguments have no
spill/reload or runtime `Bounded.from` check.

For each CPU level, original/bounded programs match status, stdout and stderr
for inputs `-1`, `0`, `1`, `2`, `3`, `10`, `100`, `1000001` and malformed text.
A separate helper probe compares all sixteen pairs from
`{0, 1, MAX_N - 1, MAX_N}` with exact scalar-formula float bits in optimized
and fast-compile modes. These controls do not run a million-square benchmark.

## Whole-program and compilation measurements

AMD Ryzen 7 7840HS, CPU 3, `n = 2000`, baseline x86-64, nine alternating
samples per comparison. Output SHA-256 hashes match for every sample.
Instruction ranges vary only with process startup; CPU ranges overlap.

| Comparison | Baseline instructions | Bounded instructions | Baseline CPU ms, median (range) | Bounded CPU ms, median (range) |
| --- | ---: | ---: | ---: | ---: |
| Previous production → bounded | 2,724.604M | 2,404.684M | 155.70 (150.44–163.41) | 154.49 (149.42–162.81) |
| Same published backend: unchecked → bounded | 2,564.684M | 2,404.684M | 155.83 (148.99–162.82) | 154.11 (147.84–161.06) |

The instruction reductions are 11.74% for adoption and 6.24% for the rewrite
alone. Both comparisons have lower bounded CPU medians; overlapping ranges do
not establish a reliable CPU speedup. `runtime-results.json` preserves every
sample and the full host/counter commands. These are whole-process costs,
including startup, input and output, with fixed input and one host/CPU.

Three alternating, uncached source-compilation samples compare the previous
backend/original benchmark with the published backend/bounded benchmark.
Instructions change from 384.931M (384.897–384.942M) to 385.577M
(385.561–385.590M), +0.168%; CPU medians are 73.19 ms (72.63–73.75) and
72.97 ms (72.94–74.45). `compile-results.json` retains the raw linker-inclusive
process measurements. Broader backend runtime and generated 1000-unit compiler
qualification are in the linked scheduling experiment; they are not repeated
or relabeled as measurements of this revision.

## Reproduce

Build a release compiler with `cargo build --locked --release --bin plenty`.
Compile and dump the benchmark with caches disabled at each CPU level:

```sh
PLENTY_CACHE=off target/release/plenty --no-cache --cpu x86-64 --compile benchmarks/game/spectral-norm/plenty/spectralnorm.plenty -o /tmp/spectral-bounded
PLENTY_CACHE=off target/release/plenty --no-cache --cpu x86-64 --dump clif-opt,asm benchmarks/game/spectral-norm/plenty/spectralnorm.plenty -o /tmp/spectral-bounded-dump
python3 -B experiments/kernels/loops.py < /tmp/spectral-bounded-dump/asm.txt
```

Repeat with `x86-64-v3`. For production comparisons, build base `58e5a09b`
in a separate worktree, using its original benchmark and lockfile; do not
replace a compiler while collecting samples. The common measurement driver is
`experiments/kernels/counted-latch/measure.py`; it alternates separately built
`NAME-baseline` and `NAME-candidate` executables, checks their output hashes,
and retains every instruction/task-clock sample. CPU ranges are measurements
on one host, not a universal speed guarantee.

## Qualification

Focused cache-disabled checks pass: 28 cache/object-identity tests, 11 bounded
argument/conversion tests, 61 loop/proof/continue tests and 23 interval tests.
Both lockfiles were parsed to verify that their only package changes are the
approved fork revision: 16 production and 13 harness packages. The locked
release harness builds and reproduces the reduced 15-instruction matrix loop.
Formatting, all-target/all-feature Clippy with `-D warnings`, source links,
377 Python tests (four skipped) and the complete published-site build pass.
The final full suite is run only after this focused qualification and
blocker-free self-review; its exact result and retained timing report are
recorded in the local PR.

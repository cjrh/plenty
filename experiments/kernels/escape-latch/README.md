# Counted escape latch (issue #977)

This follow-up isolates the original escape loop in
`../unused-range/kernels.plenty`, rather than the bounded spectral-norm scope
of `../counted-latch/`. Both compilers use Plenty `6a466afb`; the baseline uses
the published Cranelift `d27f16528ab79ae70b6e0f4caca1d5e3a787e92b` and the
candidate uses local, unpublished `7d63dc32d3` plus this PR's countdown lowering.
`cranelift.patch` preserves the complete candidate against d27. No optional CPU
features, benchmark source changes, or issue #980 fork changes are included.

## Attribution

| Kernel | C | d27 baseline | Candidate |
| --- | ---: | ---: | ---: |
| Original Plenty escape | 15 | 20 | 18 |
| CLIF a: original signed countdown | 15 | 20 | 19 |
| CLIF c: nonzero countdown | 15 | 20 | 18 |
| Unused range element | 5 | 5 | 5 |

The positive-entry guard establishes `remaining > 0`. Each path reaching the
latch executes one recognized decrement; its result is nonnegative, so `!= 0`
is equivalent to `> 0`. The initial guard, early exits and floating comparison
are unchanged. Plenty applies this only when the range element is unused and a `break` can
leave this loop (nested-loop breaks are excluded), leaving straight counted
loops and the existing single-counter lowering for used elements intact.

The x64 backend reuses an add/subtract's zero flag when a same-block branch is
its first use. Definitions in another block and results used earlier are
excluded. It also moves register-only critical-backedge copies before a branch
when their destinations are dead on the alternate successor and are not branch
inputs. That lets MachBuffer collapse the empty edge and its unconditional jump.
The copies preserve flags. Calls, safepoints, terminator definitions, debug value
locations and non-register edits are excluded. Physical liveness includes the
effective spill-forwarding edits and partial-register reads.

The three remaining instructions against C are scalar copies, issue #978.
Every steady-state instruction is accounted for:

| Category | C | Baseline | Candidate |
| --- | ---: | ---: | ---: |
| Floating arithmetic | 8 | 8 | 8 |
| Scalar register copies | 3 | 6 | 6 |
| Floating compare | 1 | 1 | 1 |
| Conditional branches | 2 | 2 | 2 |
| Countdown subtraction | 1 | 1 | 1 |
| Separate countdown test | 0 | 1 | 0 |
| Unconditional back jump | 0 | 1 | 0 |
The patch removes no overflow/bounds check, trap, floating operation, or reload.
`source-baseline.asm` and `source-candidate.asm` retain the original machine code;
`baseline.asm` and `candidate.asm` retain all twelve existing CLIF variants.
Variants a/b/e/f/g/h lose one, c loses two, d/i lose one, j is unchanged, and k/l
lose one. Negative-count variants remain controls rather than adopted lowering.

## Qualification

Both final backends pass the existing native checker: 480,000 C/CLIF comparisons
each, including signed zeros, infinities, quiet/signalling NaNs, random bit
patterns and escape/final completion. The new native Plenty regression covers
negative/zero counts, one/final iteration, early breaks, continues, a huge count
with an early break, and unordered escape inputs, in optimized and fast modes.
All 11 focused range tests pass. The final cache-disabled unrestricted full
suite passes: 315 compiler tests, 6,892 integration tests (4 ignored), 304
runtime tests (5 ignored), 11 other tests and doctests. `full-suite-timings.json`
records 45.9 seconds compilation and 274.2 seconds execution. Plenty's
all-target/all-feature Clippy (`-D warnings`) and formatting pass.

All three docs CI checks pass after restoring production manifests: source-link
checks, 377 Python tests (4 skipped), and the complete site build (1,648 default
and 824 development pages).

All 38 existing Plenty kernel/benchmark programs compile with both compilers.
`source-counts.json` preserves every loop count. The escape and Mandelbrot inner
loops fall from 20 to 18; no recorded loop grows.

A broader nonzero rewrite of every private countdown was rejected. It changed
register allocation elsewhere in large functions: ordinary n-body grew from
2,355.723M to 2,360.723M instructions (+0.212%) and its large loop from 471 to
472 instructions. Restricting the rewrite to escape loops restores the baseline
loop counts. `rejected-global-source-counts.json` and
`rejected-global-runtime-results.json` retain that control. Extending d27's
argument-first schedule to direct arithmetic tests did not remove the
regression; `rejected-argument-order.patch` preserves that rejected probe and
is not part of the candidate fork. The retained backend remains `7d63dc32d3`.

The final fork passes 420 selected x64/AArch64/egraph filetests, three egraph
unit tests, Clippy and formatting. The s390x-targeted `exponential-selects.clif`
is excluded from this x64/AArch64 build. The new precise-output regression
covers signed/nonzero escape, an earlier-use exclusion, and a cross-block
source control (the egraph may duplicate its pure subtraction before lowering). The only
existing expectation updated is `ymm-spill.clif`, where the redundant `testq`
and its three-byte offset are removed.

Allocator qualification exports the exact committed baseline/candidate with the
registry allocator and Plenty's pinned `b2dc24bb703c3bcd7e781e58e70aa4ff6d5b70d1`.
Registry runs pass 208/209 x64 files respectively. The pinned allocator retains
the same 15 existing precise-output mismatching files, with identical diagnostics
after timestamps and the corpus count are removed. No allocator expectation is
blessed. `allocator-comparison.json` preserves the diagnostics; this establishes
no new difference, not a passing pinned-allocator filetest suite.

The restored production backend pin `98c03730` also passes all 11 focused
range regressions with the frontend change; production manifests have no diff.
The full suite and performance evidence above use the isolated candidate.

## Reassessment on the production pin

After issue #978, production Cranelift `2986d9c583` emits a conditional backedge
in the standalone SSE2 reproduction and both copies of the Mandelbrot escape
loop. A separate countdown test remains. Compile the optimized Plenty CLIF dumps
with the pinned harness, then with just the arithmetic flag hunks from
`cranelift.patch` applied to that pin, and with the full patch. The counts below
come from `loops.py`; the inlined reproduction counts only the escape loop
in `unused-range/kernels.plenty`'s `main`.

| SSE2 loop | Production | Flag reuse only | Full patch |
| --- | ---: | ---: | ---: |
| Standalone reproduction | 16 | 16 | 16 |
| Inlined reproduction | 20 | 19 | 18 |
| Standalone Mandelbrot | 16 | 15 | 15 |
| Inlined Mandelbrot | 16 | 15 | 15 |

Biasing the countdown to test unsigned overflow or borrow on the production
backend reaches 15 instructions in standalone Mandelbrot, but grows the
standalone reproduction from 16 to 19. It duplicates the decrement and adds
a counter copy and back jump. Moving the decrement in the input CLIF leaves
these counts unchanged. Testing the old count against one also grows the
standalone reproduction to 19.

All twelve top-tested escape variants remain at 16 instructions on SSE2 with
either backend patch. On AVX the full patch reduces all twelve variants from
15 to 14 and the flag-only patch leaves them at 15. The C oracle passes
480,000 comparisons per backend and CPU profile. These are kernel checks
against the updated pin. The runtime measurements below used the older pin.

Issue #977 was closed as deferred. The full patch adds post-allocation
physical-register liveness analysis and edge-copy movement, and the smaller
alternatives fail to reduce the original standalone SSE2 loop. The experiment
retains the patch and its qualification if a smaller approach or a measurable
runtime benefit warrants revisiting it.

## Whole-program measurement

Three alternating baseline/candidate runs use the unchanged benchmark quick
workloads, perf user-space instruction/task-clock counters, and CPU affinity
0–7. All 84 outputs match. `runtime-results.json` retains every command, output
hash, counter and CPU sample. Mandelbrot falls from 8,202.032M to 7,465.708M
instructions (−8.98%); CPU medians are 712.91/712.39 ms with overlapping ranges
711.48–717.75/710.96–712.45 ms. This establishes the instruction reduction,
without establishing a CPU speedup. N-body is unchanged at 2,355.723M
instructions, removing the rejected broader rewrite's regression. The other
serial fixed-work controls remain effectively unchanged; parallel scheduling
and hash-table programs retain sample variation. No steady-state loop in the
38-program corpus grows.

Compilation uses three alternating runs pinned to CPU 3, with Plenty caches
disabled. `compile-results.json` retains raw samples. No meaningful compilation
speed gain is claimed: AOT CPU ranges overlap, and the small source probes have
startup/cache variation.

| Compilation probe | Baseline M instructions | Candidate M instructions | Change | CPU medians ms (baseline/candidate) |
| --- | ---: | ---: | ---: | ---: |
| aot-1000x10 | 129153.773 | 129053.721 | -0.077% | 15362.60 / 15382.31 |
| spectral | 386.389 | 386.672 | +0.073% | 71.65 / 73.83 |
| bounded | 387.186 | 387.433 | +0.064% | 76.17 / 75.56 |
| fasta | 395.684 | 395.502 | -0.046% | 76.04 / 77.73 |

## Reproduction

Use the existing harness and `../counted-latch/verify-escape.c` with each exact
backend. Build Plenty and the harness with temporary local path overrides in
isolated trees; restore manifests and lockfiles afterward. Follow
`book/src/development/workflow.md` for the fork qualification procedure.

```sh
PLENTY_KERNEL_CODE_DIR=/tmp/escape-code plenty-kernel-harness ../counted-latch/escape.clif
gcc -O2 -ffp-contract=off -Wall -Wextra -Werror ../counted-latch/verify-escape.c -o /tmp/verify-escape
/tmp/verify-escape /tmp/escape-code
plenty --no-cache --dump asm ../unused-range/kernels.plenty -o /tmp/escape-source
python3 ../loops.py < /tmp/escape-source/asm.txt
```

For whole-program and compilation measurements, retain separately built
`NAME-baseline`/`NAME-candidate` benchmark executables and frozen
`baseline-plenty`/`candidate-plenty`, `baseline-compile-bench`/`candidate-compile-bench`
pairs. Run from the repository root, with other build/test jobs stopped:

```sh
python3 experiments/kernels/counted-latch/measure.py /tmp/escape-programs --output /tmp/runtime-results.json --cpu 0-7 --repeats 3
python3 experiments/kernels/counted-latch/measure_compile.py /tmp/escape-compilers --output /tmp/compile-results.json --cpu 3 --repeats 3
```

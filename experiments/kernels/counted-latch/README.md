# Integer loop latch scheduling (issue #977)

This experiment attributes two instructions in bounded spectral-norm's inner
loops to Cranelift's egraph elaboration order. The loop condition uses the next
counter; the backedge's other arguments still use the old counter. Elaborating
the condition first increments into another register, then splits the backedge
with a counter copy and unconditional jump. Elaborating the arguments first
finishes the old counter's uses before the increment and removes both.

The candidate is the local Cranelift commit `d27f16528a`, based on the pinned
`98c03730582a07fe57c027f0e18e821040c68b8c`. `cranelift.patch` preserves its exact
source and regression tests. The original fork's branch and remote are unchanged.
Qualification passed; publication and adoption are pending; the patch is not a production claim that
all counted escape loops are fixed. Plenty's baseline is `eacaf201`.

## Kernel counts and rejected variants

AMD Ryzen 7 7840HS, baseline x86-64 ISA, optimized native compilation, GCC 16.2.1
`-O2`. Counts are steady-state instructions from `../loops.py`.

| Loop | C | Baseline | Candidate |
| --- | ---: | ---: | ---: |
| Bounded A × u | 14 | 17 | 15 |
| Bounded Aᵀ × u | 14 | 17 | 15 |
| Existing unchecked A × u | 14 | 16 | 16 |
| Existing unchecked Aᵀ × u | 14 | 18 | 16 |
| Unused range element | 5 | 5 | 5 |
| Escape | 15 | 20 | 20 |

The bounded loop has one remaining instruction against C: a separate floating
load, tracked by issue #980. The two removed instructions are the counter copy
and extra latch jump. No bounds/overflow check, trap, arithmetic operation,
negative-index join, call or reload is removed. `matrix.clif` reduces the exact
old-counter dependency to one function: 17 → 15 instructions.

`escape.clif` retains the other isolated control-flow experiments:

| Variant | Change | Baseline / candidate |
| --- | --- | ---: |
| a | Original optimized escape loop | 20 / 20 |
| b | Complement latch and put exit first | 20 / 20 |
| c | Nonzero countdown after positive entry | 20 / 20 |
| d | Zero completion and exit-first latch | 22 / 22 |
| e | Exact unordered complement of escape comparison | 20 / 20 |
| f | Complement escape and latch | 20 / 20 |
| g | Rotate compound count/escape test around recurrence | 20 / 20 |
| h | Compound rotation and zero completion | 20 / 20 |
| i | Separate result exits | 22 / 22 |
| j | Compound rotation and separate initial exit | 23 / 23 |
| k | Negative private countdown with unsigned-add carry | 19 / 19 |
| l | Compound rotation and negative-count carry | 19 / 19 |

An initial unconditional condition-last elaboration regressed floating-tested
compound variants g/h from 20 to 22 and l from 19 to 21. The retained candidate
therefore changes only integer-comparison branches with a successor dominating
the branch. Floating comparisons and forward branches retain their order.
The negative-count variants are controls, not adopted lowering: implementing
that representation would need a private-count/continue/cleanup proof. Issue
#977's original escape scope remains open, alongside issue #978's scalar-copy
scope. No loop rotation or body-only range proof is changed here.

## Semantic and compatibility checks

The existing CLIF harness uses the verifier and emits relocation-free raw code.
`verify.c` compares the reduced matrix kernels bit-for-bit on 35,880 cases:
negative/zero lengths, all lengths through 65, four row indices, random floating
bit patterns, signed zeros, infinities and quiet/signalling NaNs.
`verify-escape.c` checks all twelve variants against C on 40,000 special,
random and grid inputs each: 480,000 comparisons for each compiler. It covers
both escape and final bounded completion with unchanged floating operation order.

All 25 pre-existing kernel CLIF files compile with the verifier; their loop counts
are unchanged. All 26 Plenty kernel programs and 12 benchmark source files
compile under each compiler. No inner-loop count grows; bounded loops lose two,
existing spectral-norm's transpose loses two, and one large loop in each n-body
variant loses two. `source-counts.json` retains the complete comparison.

The final cache-disabled `just test` passes: 6882 integration tests (four
ignored), plus all workspace unit, example and doctests. Compilation took
110.1 seconds and execution 347.9 seconds; the timing report and exact temporary
path overrides are retained under `target/issue-977-validation/`. All temporary
manifest/lockfile changes were restored after qualification.

Fork checks pass: 420 egraph/x64/AArch64 filetests and three egraph unit tests.
The new integer-latch regression fails on the exact baseline backend and passes
on the candidate. Fork Clippy and formatting pass. The full docs checks also
pass: source links, 373 Python tests (four skipped), and the complete site
(1648 default and 824 development HTML pages).
The existing LICM test changes only its expected elaboration order/value names.
The allocator integration runner passes all 208 x64 files with Cranelift's
registry allocator at both revisions. Plenty's pinned allocator has the same
15 precise-output mismatching files at both revisions, with identical per-file
diagnostics; no expectation was blessed. `allocator-comparison.json` preserves
the file set. This is no new difference, not an assertion that the allocator
integration suite passes.

## Whole programs and compilation

`runtime-results.json` retains three interleaved pinned-CPU samples for every
benchmark variant, including parallel variants and the bounded spectral-norm
candidate. Input sizes are each benchmark manifest's `quick` size. Every
before/after output hash matches. All optional CPU features remain disabled.

| Program | Before / candidate, M instructions | Median CPU ms, before / candidate |
| --- | ---: | ---: |
| Bounded spectral-norm, n=2000 | 2724.527 / 2404.687 | 150.16 / 150.04 |
| Existing spectral-norm, n=2000 | 2724.604 / 2564.684 | 151.32 / 150.42 |
| N-body, 5M steps | 2365.723 / 2355.723 | 180.53 / 176.70 |
| Columnar n-body, 5M steps | 2410.690 / 2400.690 | 188.40 / 181.62 |

Other benchmark counts are unchanged apart from keyed-hash and worker scheduling
variation. k-nucleotide's process-secret SipHash key changes bucket probes;
`hash-runtime-results.json` adds nine interleaved samples per variant. Median
counts change −0.117%/−0.032%, within overlapping ranges. CPU ranges also overlap;
these samples do not establish a reliable CPU speedup.

Three interleaved compile samples, caches disabled, include children/linking.
The AOT harness includes its default single warm-up plus ten measured
repetitions:

| Probe | Before / candidate, M instructions | Change |
| --- | ---: | ---: |
| `compile_bench 1000 10 --workload aot` | 129077.084 / 129123.164 | +0.036% |
| Existing spectral-norm | 384.793 / 384.731 | −0.016% |
| Bounded spectral-norm | 385.584 / 385.549 | −0.009% |
| Fasta | 393.974 / 393.754 | −0.056% |

`compile-results.json` retains samples, CPU counts, commands and spread. The
small generated-program compile cost is measurable and accepted only alongside
the generated instruction reduction; it is not described as zero overhead.

## Reproduction

Use separate baseline and candidate worktrees/build directories. The candidate
requires the retained fork patch applied to its recorded base, or the exact
candidate commit in a local fork checkout. Follow the local path-patch procedure
in the [fork workflow](../../../book/src/development/workflow.md#cranelift-and-regalloc2-forks)
while qualifying an unpublished fork; do
not commit machine-specific paths or change the shared fork checkout. Build the
release compiler, release `compile_bench` example and existing CLIF harness for
each. Restore temporary manifests/lockfiles after the experiment.

```sh
PLENTY_KERNEL_CODE_DIR=/tmp/latch-before plenty-kernel-harness matrix.clif
PLENTY_KERNEL_CODE_DIR=/tmp/latch-after plenty-kernel-harness matrix.clif
gcc -O2 -Wall -Wextra -Werror verify.c -o /tmp/verify-latch
/tmp/verify-latch /tmp/latch-before/0-0.bin /tmp/latch-after/0-0.bin

PLENTY_KERNEL_CODE_DIR=/tmp/escape-code plenty-kernel-harness escape.clif
gcc -O2 -ffp-contract=off -Wall -Wextra -Werror verify-escape.c -o /tmp/verify-escape
/tmp/verify-escape /tmp/escape-code
```

From the Plenty root, compile benchmark executables with each compiler's
`--no-cache --compile`, naming them `BENCHMARK-SOURCESTEM-baseline` and
`BENCHMARK-SOURCESTEM-candidate` in one directory. Name the bounded candidate
`bounded-spectral-baseline/candidate` and the parallel fannkuch example
`fannkuch-redux-parallel-baseline/candidate`. Then:

```sh
python3 -B experiments/kernels/counted-latch/measure.py BINARY_DIRECTORY \
  --output runtime-results.json --cpu 3 --repeats 3
python3 -B experiments/kernels/counted-latch/measure.py BINARY_DIRECTORY \
  --output hash-runtime-results.json --cpu 3 --repeats 9 --only k-nucleotide
python3 -B experiments/kernels/counted-latch/measure_compile.py COMPILER_DIRECTORY \
  --output compile-results.json --cpu 3 --repeats 3
```

`COMPILER_DIRECTORY` contains `baseline-plenty`, `candidate-plenty`,
`baseline-compile-bench` and `candidate-compile-bench`. The scripts preserve
commands, counters and output hashes; they fail on changed output or unavailable
counters. `validation.json` identifies the saved complete build/check logs and
assembly/optimized-CLIF artifacts retained for review.

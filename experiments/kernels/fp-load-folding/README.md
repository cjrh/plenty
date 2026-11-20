# Scalar floating-load folding experiment (issue #980)

The [adoption record](../../../book/src/completed-work/performance.md#scalar-floating-load-folding-issue-980)
reports the complete compiler with the combined backend and the single-reduction
frontend guard. This page records the earlier standalone experiment.

This evidence-only experiment separates two prerequisites for folding the input
load into spectral norm's multiply. The matched Plenty/C source is in
[`bounded-arguments`](../bounded-arguments/README.md). GCC `-O2` emits 14
instructions per inner A/AT pass; Plenty at Cranelift `98c0373` emits 17.
The arithmetic remains a division, a multiply, then an add, with its original
rounding order. This experiment does not introduce a fused operation.

The backend candidate is local Wasmtime commit
`76470301a2f60737091e71d282c3371ebae5b011`, based on
`98c03730582a07fe57c027f0e18e821040c68b8c`. It has not been published or pinned.
The independent scheduling candidate is
`d27f16528ab79ae70b6e0f4caca1d5e3a787e92b`, also based on `98c0373`.
Local integration `94ed85d10c` contains both committed candidates. Production
Plenty source and harness pins are unchanged. The tested frontend
change is retained in `frontend-candidate.patch`; the backend and its tests
are in `backend-precision.patch`. Publication/adoption requires approval.
Absolute path overrides used for measurements have been removed. The patch
artifacts use zero context to avoid whitespace-only context lines; apply them
with `git apply --unidiff-zero` at their recorded baselines.

## Attribution

The load initially lives in the body and the multiply/add in a separate
latch. Cranelift's existing same-block and side-effect-color conditions forbid
sinking a load across that boundary. Separately, the sum is passed to both the
backedge and exit. Its `Multiple` use state propagates through the arithmetic
tree and marks the input load multiple-use, although both successor arguments
request the sum in a register and do not regenerate its arithmetic.

The Plenty candidate coalesces the normal fallthrough and latch only for an
optimized x86 scalar load–multiply–add reduction into a loop-carried value.
A `continue` for that loop retains its separate latch, including cleanup paths;
nested loops have their own continue targets. Rotation eligibility, guard
positions, and body-only bounds facts are unchanged. A broader coalescing
experiment was rejected: it added instructions in unrelated collection loops.
The narrow frontend change alone retains all baseline loop counts across the
38-source corpus.

The backend candidate retains every direct use count and first-result
coarsening for multi-result instructions. It limits recursive multiplicity
propagation to producers used as ordinary instruction operands. Block arguments
and exception contexts always request registers. Mixed ordinary/register-only
uses remain conservative. Existing load side-effect, same-block and adjacency
checks remain in force.

| Bounded A/AT loop | Instructions/pass |
|---|---:|
| Original split blocks, current backend | 17/17 |
| Same-block frontend change alone | 17/17 |
| Backend precision alone, split blocks | 17/17 |
| Same-block frontend and precision | 16/16 |
| Scheduling candidate alone (#977), original frontend | 15/15 |
| Explicit scheduling + precision integration, same-block frontend | 14/14 |
| Matched C | 14/14 |

The final row for the combined candidate is CLIF/native integration evidence,
not a claim about a full combined Plenty compiler. The standalone actual
compiler matches 16/16. Its 38-source comparison also finds tradeoffs: unchecked
spectral norm changes A/AT from 16/18 to 17/17, and one small clist n-body loop
from 13 to 14. Whole-program measurements must assess these; no general
per-loop nonregression or CPU speedup is claimed here.

## Whole-program and compiler measurements

Three alternating baseline/candidate samples were pinned to CPU 3 of an AMD
Ryzen 7 7840HS. Programs were optimized native executables; compilation probes
used `PLENTY_CACHE=off` and `--no-cache`. The baseline compiler is Plenty
`eacaf201` with `98c0373`; the candidate includes the narrow frontend and
`7647030`, without scheduling candidate #977. Fourteen benchmark output hashes
match. Instructions are user-mode retired instructions; CPU time is user-mode
`task-clock`, not wall time. Medians are:

| Workload | Instructions change | CPU-time change |
|---|---:|---:|
| Bounded spectral norm | -5.8726% | -0.0733% |
| Original spectral norm | -0.0029% | +0.4371% |
| Clist n-body | approximately zero | +0.1755% |
| AOT compile benchmark, 1,000 functions x 10 | +0.2787% | +0.0842% |
| Compile original spectral norm | +0.2324% | +0.2797% |
| Compile bounded spectral norm | +0.2272% | -0.5240% |
| Compile fasta | +0.2514% | +0.2167% |

The bounded instruction improvement is reproducible, but these samples show
no corresponding CPU improvement. The original spectral loop tradeoff cancels
at whole-program instruction level. Other runtime instruction counts are
essentially unchanged; dictionary/hash scheduling can vary between executions.
CPU changes of a few percent in short cases are not attributed to this patch.
[Counter samples](measurement-samples.json) retain every sample and executable
and driver hashes; complete `perf` output remains in the task evidence directory.

The drivers are frozen at Plenty `8de85c55`, paths
`experiments/kernels/counted-latch/measure.py` and `measure_compile.py`, from local PR #1268, the
independent #977 experiment. Reproducing this exact automated measurement uses
that frozen driver; it is an evidence dependency, not a production dependency. They consume explicit baseline/candidate binary
pairs, alternate order, and reject mismatched program output. The recorded
combined 14/14 loop count has not been measured as a complete combined compiler
or included in the runtime table.

## Reproduce the machine-code comparison

Build separately retained harness executables for the original pin and the
candidate using the [kernel harness](../README.md). For the candidate, use the
repository's documented temporary fork override, with `PLENTY_CACHE=off` for
all compiler runs. Apply the frontend patch to the recorded baseline and the
backend patch to the recorded fork baseline. Remove the override before handing
off a Plenty PR.

```sh
BASELINE_HARNESS experiments/kernels/fp-load-folding/baseline-split.clif \
  experiments/kernels/fp-load-folding/merged-body-latch.clif > baseline.txt
CANDIDATE_HARNESS experiments/kernels/fp-load-folding/baseline-split.clif \
  experiments/kernels/fp-load-folding/merged-body-latch.clif > candidate.txt
python3 experiments/kernels/loops.py -v < candidate.txt
```

`load-in-latch.clif` moves only the load. `unrotated*.clif` are semantically
valid controls, not adopted frontend lowering: reverting rotation would also
require rebuilding its bounds-fact discipline. A diagnostic variant that passed
the old sum to the exit was excluded because it changes output.

## Native safety checks

The recorded `*-coalesced.clif` files are actual optimized matrix IR from the
narrow frontend plus backend precision. The verifier driver exports raw machine
code using each explicit harness and links it to `verify.c`:

```sh
python3 -B experiments/kernels/fp-load-folding/verify.py \
  --baseline-harness BASELINE_HARNESS --candidate-harness CANDIDATE_HARNESS \
  --output target/fp-load-verification
```

The Linux x86-64 calling-convention probe checks 2,048 bit-exact matrix cases:
four rounding modes; signed zeros, subnormals, extrema, infinities, quiet and
signaling NaNs and random bit patterns; all floating exception flags; read-only
input pages, protected span ends and output-prefix canaries. Empty spans point
at protected pages. An over-limit size must return before touching invalid
descriptors. Both precision-only and explicit combined backends pass.

Only the cold source-diagnostic helper is replaced with a trap for raw linking.
These checks exercise valid descriptors and cannot establish actual trap
messages or source borrow diagnostics. The focused loop suite separately checks
empty-loop bounds facts, guard/latch failure positions, checked arithmetic,
continue targets, nested loops and shared cleanup. A 62-test loop run, the strengthened f32/f64 cleanup cases, explicit
optimized/fast-compile probes, 68 exact trap-location tests, alias/memory-order
regressions and the new continue-traversal unit pass.

The precision change also passes six backend use-state units, including
verifier-checked mixed operand, shared load, block-only and exception-context
cases, and all native-ISA filetests: 209 x64, 134 AArch64, 239 RISC-V64 and
102 s390x. The allocator integration check reports 15 precise-output mismatches
at both original and candidate pins. The full diagnostic bodies are identical
when timestamped concurrent logger lines and the added test count are ignored;
no expectation was blessed. This is a reported nonzero check, not an all-green
allocator compatibility claim.

Raw dumps, exact source comparisons, native artifacts, original allocator
reports, tool logs and measurements are retained in `target/issue980/` in the
task worktree. The actual candidate passes final uncached `just test`: 6,884
integration tests (four ignored), 316 compiler units and all runtime/workspace
checks; 423.7 seconds total, comprising 144.8 seconds compilation and 278.8
seconds execution. The report is
`target/check-timings/suite-20261114T205344-1825996/`. Clippy and formatting pass.
Final docs CI passes after restoring the production manifests: source links,
373 workflow tests (four skipped), and the complete published site
(1,648 default and 824 development HTML pages). The first
workflow-unit invocation under path overrides failed three git-pin-parser tests;
those helpers intentionally require a real locked revision and were not weakened.
Publication/adoption remains open; no production dependency pin was changed.

# Dense-match dispatch measurement

This B19 experiment measures 4, 16, 64 and 256 dense integer/enum cases with
uniform, 90%-first and 90%-last input distributions. It compares production
comparison chains with a same-language balanced integer decision tree and a
temporary Cranelift `Switch` prototype. Production lowering is unchanged.

The measured results support optimizing large dense matches, but not replacing
all chains: four- and sixteen-arm uniform cases became slower with the switch.
The production follow-up is local issue #395. See the
[profiling report](../../book/src/development/profiling.md#dense-match-dispatch).

## Reproduce

Use a disposable worktree at `9c994af0a6476a945e3ac01e6c921dd12916bce5` with this
directory copied in, or a worktree of the benchmark PR. All commands run from
that worktree. Choose an available CPU and keep its SMT sibling idle. The
recorded machine was an AMD Ryzen 7 7840HS, Linux x86-64, CPU 2 (sibling 3),
Cranelift 0.131.1. Other agents paused heavy jobs for the runtime measurements;
the machine was not otherwise reserved or frequency-locked.

```sh
cargo build --locked --release --bin plenty
python3 examples/dense_match/measure.py --compiler target/release/plenty \
    --output /tmp/dense-baseline --cpu 2
git apply examples/dense_match/switch-experiment.patch
cargo build --locked --release --bin plenty
PLENTY_DENSE_SWITCH=1 python3 examples/dense_match/measure.py \
    --compiler target/release/plenty --output /tmp/dense-switch --cpu 2 \
    --baseline-output /tmp/dense-baseline
git apply -R examples/dense_match/switch-experiment.patch
```

The patch is measurement-only. It accepts zero-based, source-ordered, contiguous
64-bit integer cases (including lowered enum tags), with an optional final
wildcard. It retains the wildcard or trap fallback and emits `br_table` through
Cranelift's switch helper. It is not a production dispatch-selection policy and
does not support arbitrary signed, offset, sparse or reordered patterns.
`PLENTY_DENSE_SWITCH` is not included in cache keys: **always bypass the caches
when using the patch**, as the harness does with `--no-cache` and `--dump`.
Restore the source after measuring; rebuild before using that compiler normally.

For a quick generator/patch smoke check, add
`--sizes 4 --iterations 100 --repeats 1 --compile-repeats 1` to both runs.

## What is measured

- Every generated program checks every arm. Integer programs also check the
  first out-of-range value and `u64.MAX`. A separate Python implementation
  computes the expected checksum for every timed input sequence.
- Each program performs two million dispatches driven by the high bits of a
  wrapping LCG. The first/last distributions override the index on 90% of
  iterations. Return constants vary by arm rather than being the arm number.
- Enum variants are constructed in a list before the loop. Each dispatch
  borrows an indexed variant; enum totals include that indexing and borrowing
  overhead. Integer and enum absolute times are not an ABI-equivalent comparison.
- The balanced tree is a generated Plenty `if` tree with the same integer
  fallback and return values. It is a control, not a recommended source rewrite.
- Each executable warms once before timing. Seven rounds shuffle all executables
  with a fixed seed. `--baseline-output` adds the earlier baseline binaries to
  those same rounds, verifies matching source hashes/iteration counts, and
  replaces their runtime samples. These are paired-session measurements, not
  before/after measurements separated by a compiler rebuild.
- Timings are process wall time, including startup, all-arm checks and output;
  no baseline cost is subtracted. Runtime calls are not isolated cycle counts.
- Three cold compile/link samples per program use `--release --no-cache` and
  inherit the same one-CPU affinity. They include validation code, enum/list
  construction, frontend work and linking. Baseline and prototype compile
  samples were collected in separate batches, so small differences are noise.
- `--dump clif-opt,asm --dump-filter dispatch` captures the isolated dispatch
  function for uniform fixtures. Its index records bytes and instruction counts.
  Bytes include jump-table data/padding; they are not executed instruction counts.

The output directory retains source files, executables, dumps and JSON. The
checked-in `results.json` preserves all samples, commands, checksums, source and
compiler hashes, and dispatch indexes. Its baseline runtime samples were rerun
in the switch session; baseline compile samples remain from the first session.
The unchanged tree controls stayed close across the two compiler binaries.

## Limits and next step

This isolates a synthetic dispatch cost. Constant-return arms are cheap and
could also suit a data-lookup-table optimization; useful application gains may
be smaller when arm bodies dominate. No cache, allocation, peak-memory or
hardware-counter benefit is claimed. Payload-bearing/nested enums, or-patterns,
signed and narrow integers, sparse keys, representative application workloads
and additional CPUs require follow-up coverage before choosing a production
policy. Small-match regressions mean a single unconditional switch is unsuitable.

The benchmark and documentation are the deliverable. The patch remains a
reproducible experiment, and issue #395 stays open for the production change.

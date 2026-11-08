# Immutable captures in conditional callbacks — issue #891

Baseline: local main `f7515620`, after the column zip fix in PR #1115.
Plenty 0.8.19, Rust 1.99.1, Cranelift/regalloc2 0.137.0/0.15.2,
x86-64 Linux, AMD Ryzen 7 7840HS, scheduler-default affinity. Optimized
Plenty builds only, no allocation instrumentation. Measurements count
instructions; concurrent builds make elapsed times unsuitable.

The runnable kernel is `../clist-zip/scan.plenty`: the same selected-field
calculation as an explicit zip loop, conditional fold with a runtime captured
threshold, and indexed loop. Scalar C is `../clist-zip/scan.c`, compiled with
gcc 16.2.1 `-O2 -fno-tree-vectorize -fno-tree-loop-distribute-patterns`.
The whole program creates 8,192 rows and repeats all three kernels 200 times.
Both compiler revisions print `2808000.0`.

## Cause and legality

Scalar leaf callbacks already mirrored immutable owned scalar captures in SSA.
The conditional callback is represented by a `Match` and uses general call-site
inlining instead. That path reloaded the closure address from its local slot
and then the captured `limit` from the environment on every row.

The fix extends the existing snapshot eligibility to generally inlined bodies.
Their capture arguments remain real references. Within that callee, a direct
read through an immutable capture argument can use its SSA snapshot. Other
reference uses and calls still receive the original reference. This changes
neither callback evaluation order nor its branches, operations, effects or traps.
Captures must all be owned immutable scalars. Mutable, borrowed and one-shot
closures remain ineligible, as does any local exclusively borrowed anywhere
in its function, because an alias could replace its environment. Assignments
and control-flow joins continue to define the existing mirror variables.

`semantics.plenty` runs in both compilation modes and checks moved snapshots,
branch/loop assignments, exclusive replacement, construction-time snapshots,
mutable and borrowed captures, mixed float/integer/bool captures, NaN and
signed zero. Existing pipeline tests cover callback effects, trapping arithmetic,
imported sites and cache invalidation. No frontend rule or inlining size bound
is broadened.

## IR experiment before compiler edits

`variants.clif` retains the baseline optimized conditional-fold IR and a
single-change variant replacing the threshold load with its construction-time
SSA value. The remaining now-dead address work is left for Cranelift to remove.
The harness uses the same release flags as Plenty.

| Variant | Change | Static innermost loop span |
| --- | --- | ---: |
| `a_capture_loads` | Baseline dump | 10 |
| `b_capture_ssa` | Replace only the threshold load with the captured SSA value | 8 |

## Result and attribution

| Kernel | Before | After | Scalar C |
| --- | ---: | ---: | ---: |
| Explicit zip | 8 | 8 | 7 |
| Conditional captured fold | 10 | 8 | 7 |
| Indexed fields | 8 | 8 | 7 |

The actual inlined caller loops also have 8, 8 and 8 instructions. Before and
after, these loops contain no bounds checks, negative-index normalization,
overflow checks, calls or spills. Fold's two removed instructions are the
closure-address reload and captured-threshold reload. The resulting matching
path has two field loads, comparison, branch, addition, index increment, end
comparison and back edge. C uses a memory operand for the weight in `addsd`,
accounting for the remaining instruction; this work does not change that shape.
`loops.py` reports static back-edge spans, not dynamic path counts or elapsed
speedups. The isolated matching fold path has the same 10-to-8 count here.

Whole-program `instructions:u`, three runs, reported variation 0.00%:

| Before mean | After mean | Change |
| ---: | ---: | ---: |
| 44,158,729 | 40,881,853 | -7.42% |

Counts include setup, allocation, all three kernels, printing and cleanup.
They do not establish a general list-versus-clist speedup or cache-locality gain.

## Reproduce

Save a release compiler from each revision as `$BEFORE` and `$AFTER`. Run from
the repository root, with separate worktrees for the two revisions:

```sh
cargo build --locked --release --bin plenty
PLENTY_CACHE=off "$BEFORE" --dump ops,clif-opt,asm experiments/kernels/clist-zip/scan.plenty -o /tmp/capture-before
PLENTY_CACHE=off "$AFTER" --dump ops,clif-opt,asm experiments/kernels/clist-zip/scan.plenty -o /tmp/capture-after
python3 experiments/kernels/loops.py < /tmp/capture-before/asm.txt
python3 experiments/kernels/loops.py < /tmp/capture-after/asm.txt
cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml --target-dir target
target/release/plenty-kernel-harness experiments/kernels/scalar-captures/variants.clif > /tmp/capture-variants.txt
python3 experiments/kernels/loops.py < /tmp/capture-variants.txt
PLENTY_CACHE=off "$BEFORE" --compile experiments/kernels/clist-zip/scan.plenty -o /tmp/capture-before-program
PLENTY_CACHE=off "$AFTER" --compile experiments/kernels/clist-zip/scan.plenty -o /tmp/capture-after-program
perf stat -r 3 -x, -e instructions:u /tmp/capture-before-program
perf stat -r 3 -x, -e instructions:u /tmp/capture-after-program
PLENTY_CACHE=off "$AFTER" experiments/kernels/scalar-captures/semantics.plenty
PLENTY_CACHE=off "$AFTER" --fast-compile experiments/kernels/scalar-captures/semantics.plenty
```

The portable regression test checks that the optimized fold has only its two
field loads, with no threshold load. Compiler/program/source hashes and the
numeric results are retained in `results.json`.

## Compiler cost

The 1,000-unit AOT workload exported by
`experiments/kernels/clist-zip/compile_workload.py` has ten separate CLI
compilations per revision:

| Compilation workload | Before instructions | After instructions | Runs | Reported variation, before/after |
| --- | ---: | ---: | ---: | --- |
| 1,000 units, CLI | 11,633,390,139 | 11,639,814,817 | 10 | 0.01% / 0.01% |
| `fannkuch_redux.plenty` | 379,918,448 | 378,490,111 | 3 | 0.35% / 0.46% |
| `set_bench.plenty` | 363,813,773 | 363,939,357 | 3 | 0.52% / 0.20% |

The large synthetic compilation increases 0.055%, a small instruction cost.
The example changes are within their reported variation. The library AOT
runner averages 115,184,288,577 instructions across three runs (0.04% reported
variation). Its total covers ten compilations in one process and should not be
compared directly with a single CLI compilation; only the CLI figures form a
matched before/after comparison.

```sh
python3 experiments/kernels/clist-zip/compile_workload.py > /tmp/capture-compile.plenty
perf stat -r 10 -x, -e instructions:u env PLENTY_CACHE=off "$BEFORE" --compile /tmp/capture-compile.plenty -o /tmp/capture-compile-before
perf stat -r 10 -x, -e instructions:u env PLENTY_CACHE=off "$AFTER" --compile /tmp/capture-compile.plenty -o /tmp/capture-compile-after
# Repeat with examples/fannkuch_redux.plenty and examples/set_bench.plenty, -r 3.
cargo build --locked --release --example compile_bench
perf stat -r 3 -x, -e instructions:u env PLENTY_CACHE=off target/release/examples/compile_bench 1000 10 --workload aot --no-warmup
```

No pinned backend is patched. The remaining C difference is the separate
field load described above, already present in the explicit loop and recorded
in the column-zip evidence. No new unresolved capture defect is identified.

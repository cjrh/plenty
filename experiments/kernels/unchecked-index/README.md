# Assumed indexing (issue #764)

`kernels.plenty` isolates an indirect gather through two slice views. The caller
provides nonnegative indices within each view. `kernels.c` has the same
preconditions and scalar work, with no `restrict` qualification. This is an
explicit language-contract change, not bounds-check elimination based on a
compiler proof: the user decision in issue #764 makes invalid indices undefined
only in plain `unchecked:` suites compiled with `--unchecked assume`.

Measurements use optimized x86-64 Linux builds, baseline Plenty `4239252a`,
Cranelift 0.137.0, and gcc `-O2 -fno-tree-loop-distribute-patterns`.

| Kernel build | Instructions per hot pass |
| --- | --- |
| C | 5 |
| Plenty before, loop version with outer view bound proven | 12 |
| Plenty before, fallback loop with both checks | 14 |
| Plenty after | 9 |

Attribution against the 5-instruction C loop:

| Cause | Before (12-instruction loop) | After | Explanation |
| --- | --- | --- | --- |
| Bounds check | 2 | 0 | `cmp` and branch on the gathered index |
| Negative-index address join | 1 | 0 | Separate `lea` before the joined address; the negative fallback is cold |
| Float load representation | 2 | 2 | Integer load and GPR-to-XMM `movq` before `addsd`; C folds the load into `addsd` |
| Loop layout/counting | 2 | 2 | Exit-first loop plus unconditional backedge and separate countdown/index update |
| Useful work | 5 | 5 | Two loads, addition and iteration control, folded differently by C |

No hot-path overflow checks, header reloads, spills, or calls remain in this
view kernel. The cold negative-index normalization is entirely absent after
the change. Remaining float-load work is issue #812; loop layout is issue #726.

The CLIF files retain the dumped baseline's function identifiers and comments.
Run each separately through the [existing harness](../README.md), because the
loop counter groups functions by identifier:

| CLIF variant | Change | Instructions per pass |
| --- | --- | --- |
| `checked.clif` | Baseline optimized IR | 12 / 14 |
| `assumed.clif` | Replace the individual index-check branches with direct jumps; keep the original loop-version choice | 9 / 9 |
| `lowered.clif` | Actual updated lowering; no speculative loop-version choice is needed | 9 |
| `float-load.clif` | Experiment only: load f64 directly instead of integer load plus bitcast | 7 |

The float-load experiment is not part of the compiler change.

```sh
cargo build --locked --release --bin plenty
PLENTY_CACHE=off target/release/plenty --unchecked assume \
  --dump ops,clif-opt,asm --dump-filter gather \
  experiments/kernels/unchecked-index/kernels.plenty -o /tmp/index-dump
gcc -O2 -fno-tree-loop-distribute-patterns -c \
  experiments/kernels/unchecked-index/kernels.c -o /tmp/index-c.o
objdump -d --no-show-raw-insn /tmp/index-c.o | python3 experiments/kernels/loops.py
cargo build --locked --release --manifest-path experiments/kernels/harness/Cargo.toml \
  --target-dir target/kernel-harness
target/kernel-harness/release/plenty-kernel-harness \
  experiments/kernels/unchecked-index/lowered.clif | python3 experiments/kernels/loops.py
```

## Whole program

At 1,000,000 steps, `benchmarks/game/n-body/plenty/nbody_unchecked.plenty`
compiled with `--unchecked assume` and the benchmark's Rust reference both
print `-0.169075164` and `-0.169086185`.

| Build | User instructions |
| --- | --- |
| Plenty before | 2,595,337,430 |
| Plenty after | 1,603,320,288 |
| Rust reference, benchmark recipe | 229,303,035 |

The change removes about 38.2% of Plenty's instructions; it remains about
7.0 times the reference. This change does not address that entire gap.

Compiling the same n-body source with the cache disabled, three repetitions of
`perf stat -e instructions:u`, used 400.31M instructions before (reported
spread ±0.32%) and 388.68M after (±0.35%). Wall-clock time on this shared host
was not used as a performance result.

The synthetic 1,000-variable AOT workload used by `compile_bench`, compiled
through the CLI with caching disabled, used 11,018.31M instructions before
and 10,927.31M after (three repetitions, reported spread ±0.01% each).
This checks compiler work independently of the improved generated program.

The repository benchmark command
`target/release/examples/compile_bench 1000 10 --workload aot` completed with
optimized code generation: 119,899,530,427 user instructions for the process
(including warmup and all ten repetitions). This is an after-only harness
check; the paired CLI measurements above provide the comparison.

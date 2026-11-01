# Serial pipeline measurements

Historical measurements; see [Profiling the compiler](profiling.md) for the profiling guide.

## Serial pipeline callbacks

`scripts/bench_pipelines.py` compares an inline range/filter/map/fold loop with
fused pipelines using named callbacks, closure literals, and immutable scalar
captures. All variants do the same checked arithmetic: filter even values,
square each accepted value modulo 1,000, and add the results. A second pair
compares a loop with a closure-literal `map` whose body is a conditional
expression, against its own inline loop. Run:

```sh
python3 -B scripts/bench_pipelines.py target/debug/plenty --items 100000000 --runs 7
```

The compiler executable can be a debug build: the harness requests optimized
native output with `--release`, disables caches, checks every result, interleaves
executables and reverses their order on alternate rounds. Compilation and cold
checking of 100 separate functions are measured separately. This is a repeatable
measurement tool, not a timing-sensitive unit test.

Issue #399 recorded the original named-callback pipeline at 1.906 times its
inline-loop baseline. A fresh pre-change run on this host measured 0.160906 s
for the inline loop and 0.283777 s for the pipeline (1.764 times, medians of
seven executions over 100 million items). The direct callback calls survived
fusion. Bounded scalar leaf substitution removes those calls, while SSA mirrors
of immutable scalar captures expose constant divisors to native optimization.

Only straight-line scalar leaf bodies with at most 24 meaningful operations
qualify. Capture mirrors follow local assignments and joins; exclusive borrowing
disables them for the affected local. Checked arithmetic keeps the original
callee source location. Debug code and larger or effectful callbacks retain
their existing behavior. Broader callback substitution needs separate evidence.

On the same host, a quiet post-change run over 100 million items (seven
interleaved executions per variant) measured:

| Variant | Native median | Relative to inline loop | Cold check, 100 functions | Single-program compile |
|---|---:|---:|---:|---:|
| Inline loop | 0.156573 s | 1.000 | 0.011199 s | 0.046002 s |
| Named callbacks | 0.155743 s | 0.995 | 0.011768 s | 0.043120 s |
| Closure literals | 0.169209 s | 1.081 | 0.023402 s | 0.042156 s |
| Scalar captures | 0.154756 s | 0.988 | 0.023753 s | 0.044352 s |

All executions returned `24500000000`; all three pipeline forms meet the
proposal's within-10% runtime target for this workload. Closure literals still
cost about 8% here, and checking 100 closure-heavy functions costs roughly twice
the named-function variant. Single-program compilation is one observation per
variant, not evidence of a compilation speedup. The machine was not frequency-
locked or CPU-pinned; small runtime differences around parity are noise. These
results do not promise the same ratios for larger, branching or mutable callbacks.

### Closure-literal splicing

Closure literals written in a chain are now lowered in the consumer's loop
rather than as hidden functions (B75 stage 2). Before timing, `--dump ops` showed
no `ClosureNew` or `ClosureCall` in the fused functions, and the optimized asm of
the filter/map/fold closure variant has no call inside the loop. The branching
variant's asm has 107 instructions against 108 for its inline loop.

Both compilers were measured on this host back to back, pinned with
`taskset -c 0,2,4,6,8,10,12,14`, over 100 million items with seven interleaved
executions per variant:

| Variant | Before: median | Before: ratio | After: median | After: ratio |
|---|---:|---:|---:|---:|
| Inline loop | 0.155813 s | 1.000 | 0.157660 s | 1.000 |
| Named callbacks | 0.157610 s | 1.012 | 0.156564 s | 0.993 |
| Closure literals | 0.159477 s | 1.024 | 0.156964 s | 0.996 |
| Scalar captures | 0.159048 s | 1.021 | 0.156939 s | 0.995 |
| Branching loop | 0.223247 s | 1.000 | 0.222619 s | 1.000 |
| Branching closure | 0.234639 s | 1.051 | 0.225732 s | 1.014 |

Cold checks of 100 functions, medians of seven runs:

| Variant | Before | After |
|---|---:|---:|
| Inline loop | 0.003607 s | 0.004034 s |
| Named callbacks | 0.003654 s | 0.003728 s |
| Closure literals | 0.005867 s | 0.005057 s |
| Scalar captures | 0.007005 s | 0.004893 s |
| Branching loop | 0.004116 s | 0.003629 s |
| Branching closure | 0.005514 s | 0.004041 s |

All runs returned `24500000000` and `16850000993`. Closure literals now run
within 1.5% of their inline loops. Checking closure-heavy functions fell from
about 1.6 to 1.4 times the named variant; the remainder is per-literal capture
and splice lowering. Cold-check numbers are one series per compiler and vary by
roughly 10% between runs.

## Fluent generators and explicit conversion

`scripts/bench_pipeline_generators.py` measures the same filter/map/fold workload
with range and generator sources, local fused consumption, explicit `.generator()`
conversion. The compiler executable requests
optimized native output, disables caches, checks equal results, and alternates
execution order as in the [callback benchmark](#serial-pipeline-callbacks):

```sh
python3 -B scripts/bench_pipeline_generators.py target/debug/plenty --items 5000000 --runs 7
```

A quiet run on this host measured the following medians. Every execution returned
`1225000000`.

| Variant | Native median |
|---|---:|
| Inline range loop | 0.010011 s |
| Local fused range pipeline | 0.009781 s |
| Inline generator loop | 0.042060 s |
| Generator source through `iter(...)` | 0.044602 s |
| Direct fluent generator source | 0.044454 s |
| Range pipeline converted with `.generator()` | 0.027202 s |
| Generator-source pipeline converted with `.generator()` | 0.065896 s |
| `std.iter` chain with the same generator source (removed module; see the discussion after the table) | 0.105389 s |

Direct fluent syntax has the same cost as wrapping the source in `iter(...)`
(0.997 times here), and costs 1.057 times the inline generator loop. Explicit
conversion adds a resumable frame: the converted range pipeline costs 2.781 times
local consumption. With the same generator source, the converted pipeline took
0.625 times the runtime of a nested `std.iter` adapter chain. That row was
measured at commit `57982e0f`; `std.iter` has since been removed, and the script
no longer runs it. Internal filter/map adapters are fused
into one frame; resuming the source generator and the converted frame still has
a cost. Keep local consumers fused when the stream does not need to escape.

The existing 100-million-item callback benchmark was also rerun with seven
interleaved executions. Named callbacks, closure literals and scalar captures
measured 0.999, 1.066 and 1.000 times the inline loop respectively, retaining the
within-10% target. Compiler tests separately check a single converted frame with
inlined scalar callbacks, and zero tracked allocations while constructing,
moving, resuming and dropping an owned scalar pipeline. These timings are
observations on a machine without frequency locking or CPU pinning; they do not
establish universal ratios or allocation behavior for heap-owning source items.

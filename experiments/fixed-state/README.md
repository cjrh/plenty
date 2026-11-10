# Automatic bounded numerical state (issue #930)

The compiler now recovers scalar state from the **unchanged original n-body
program**. At one million steps, instruction count falls from 1,995.713M to
702.713M (64.8% fewer), and median time in this session falls from 104.89ms to
51.82ms (2.02x faster). This is a compiler optimization, not a benchmark rewrite.

The implementation is `src/codegen/fixed_state.rs`. The
[reference](../../book/src/design/toolchain/02-architecture/numerical-loops.md)
describes its guards, eligibility, budgets and preserved contracts. A literal
capacity is a type-scoped **hint**, never a proof of length. Runtime guards prove
scalar facts and all accessed bounds. The original loop remains the fallback.
The pass keeps allocations and their failure order, preserves floating-point
expression order, and refuses calls, escaping references, resize operations,
unsupported control flow and arithmetic that might fail in the fast body.

## Whole-program comparison

Ryzen 7 7840HS, x86-64 Linux; Plenty 0.8.25, Cranelift 0.137.0; baseline compiler
`c8d502b6b4f738cadb41bbf7ef995974857c4125`. Three instruction observations and
nine rotating timing rounds, pinned core 0, one million steps. Counts include
startup and full-state-output dispatch, but timing runs print only the two
ordinary energy lines. [Raw observations](results.json) retain every sample.

| Program | Median M instructions | Median ms | Range ms |
|---|---:|---:|---:|
| Original Plenty, before | 1,995.713 | 104.89 | 102.70–116.28 |
| Same source, automatic scalar state | 702.713 | 51.82 | 50.76–55.47 |
| Rust reference, baseline x86-64 | 451.636 | 35.61 | 34.55–37.26 |
| Rust reference, native target | 229.636 | 28.04 | 27.53–29.91 |
| Clang fixed locals, baseline | 334.497 | 32.90 | 32.51–34.61 |
| Clang fixed locals, native target | 227.497 | 31.11 | 30.90–33.02 |

The C/Rust executables are the retained, verified builds from the
[structural study](../nbody-structural/README.md), **rerun in this session**.
C is an explicitly expanded diagnostic kernel; Rust is the reference program.
Native targeting changes the ISA and is not Plenty's current default.
The remaining time ratio to native Rust is 1.85x; the baseline-ISA instruction
ratio to Rust is 1.56x. Timings were collected on a shared development machine,
so the direction and scale are more reliable than small differences.

The final owner-provenance correction leaves `advance` assembly byte-identical.
[Confirmation samples](confirmation.json) retain the final compiler's counts.
That repeat overlapped the full suite: absolute medians rose to 205.27ms before
and 100.21ms after, while instruction counts and the roughly 2x timing gain stayed
stable. The table above uses the quieter paired session, not mixed observations.

## Emitted loop and attribution

The fast timestep is one backedge over scalar SSA values. There are no list
accesses, bounds checks, negative-index normalization, overflow checks, calls,
or inner-loop branches in it. Thirty-five body fields and forty scratch cells
are promoted; register allocation still spills numerical values to the stack.
The existing `experiments/kernels/loops.py` parser identifies the backedge and
`experiments/nbody-structural/ledger.py` classifies every instruction. The full
before/fallback inner-loop ledger and final loop are in [results.json](results.json).
The fallback retains the original staged list loops and their checks.

| Per-timestep instructions | Automatic scalar state | Explicit scalar source target¹ | Scalar CLIF¹ | Clang scalar¹ | Clang vectorized¹ |
|---|---:|---:|---:|---:|---:|
| Scalar FP arithmetic | 280 | 280 | 280 | 280 | 26 |
| Packed FP arithmetic | 0 | 0 | 0 | 0 | 129 |
| Stack moves | 254 | 245 | 232 | 98 | 53 |
| Register copies | 153 | 132 | 119 | 97 | 74 |
| Lane rearrangements | 0 | 0 | 0 | 0 | 44 |
| Other memory moves | 0 | 0 | 0 | 8 | 5 |
| Zeroing | 10 | 10 | 10 | 10 | 1 |
| Integer/control | 5 | 5 | 4 | 2 | 2 |
| **Total** | **702** | **672** | **645** | **495** | **334** |

¹ Targets and their original machine-code ledger are retained in the structural
study, not newly invented source transformations. The automatic pass preserves
the original staged algorithm; it does not fuse the pair loops. The retained [staged scalar-source generator](staged.py) also reaches 672
instructions with the baseline compiler.
The additional 30 instructions versus the explicit scalar target are nine stack
moves and 21 register copies. Relative to scalar Clang, the 207-instruction gap
is 156 stack moves, 56 register copies and three control instructions, offset
by eight fewer non-stack moves. Both execute the same 280 FP operations.
Automatic SIMD packing and backend scheduling/register use remain issue #930.

## Guard variant and compilation cost

| Variant | Hot-loop instructions | n-body compilation M instructions | `advance` bytes |
|---|---:|---:|---:|
| Original | about 1,995 dynamically | 430.069 | 6,259 |
| Scalar state, duplicate checked entry/exit accesses | 691 | 550.901 | 24,350 |
| Scalar state, entry/exit accesses proved by guards | 702 | 481.726 | 14,286 |

The [intermediate observations](checked-entry-results.json) retain the checked
variant. Removing checks that the dominating length guards already prove reduces
compilation and code size, but changes register allocation: twelve fewer stack
moves and 23 more register copies in the loop. The final tradeoff keeps the large
runtime gain with materially lower compiler/code-size cost. It changes no safety
contract: a failed size guard reaches the original checked loop.

Compiler measurements use optimized release compilers, `PLENTY_CACHE=off`, three
`perf instructions:u` runs pinned to core 0, including checking and linking:

| Source | Before M instructions (range) | After M instructions (range) |
|---|---:|---:|
| Original n-body | 430.069 (430.064–430.074) | 481.726 (481.708–481.739) |
| k-nucleotide | 420.444 (420.441–420.506) | 418.677 (418.671–418.692) |
| spectral-norm | 379.509 (379.488–379.510) | 378.067 (378.041–378.075) |

n-body compilation grows 12.0%; its `advance` function is 2.28x the original size
because it includes both implementations. The other two programs show no
compilation regression in this sample; this is not a claim of a runtime gain
for them. `compile_bench 1000 10 --workload aot` used 127.824 billion user
instructions including warmup and linking. Its 1,165.96ms median overlapped the full suite and is an
unpaired compiler timing observation, not a comparison with the baseline compiler.

## Validation and reproduction

- Eight typed-IR tests verify actual promotion of the original benchmark,
  differently named records, sizes 2/3/6, f32, and scratch state; disabled
  optimization, conflicting/large size hints, calls, potentially failing integer
  work, multiple borrowed roots and resizing decline promotion. A moved owner remains
  rooted in its destination slot. Transformed IR
  is independently type-checked.
- Sixty executable cases compare optimized and fallback output, exit status and
  exact diagnostics across changed lengths, short scratch lists and empty ranges.
- The original benchmark's two energy lines and all 35 fields match exactly at
  -1, 0, 1, 2, 3, 10, 1,000, 10,000 and 1,000,000 steps. Integration coverage
  additionally compares optimized with unoptimized execution.
- [Random-state verification](verification.json): sixteen deterministic initial
  states, five step counts each, **80 exact full-state comparisons**.
- Allocation-failure injection confirms scratch allocation still fails before
  the loop, without modifying the body state.

```sh
# Preserve the baseline release compiler before applying this change.
RUSTC_WRAPPER= cargo build --offline --locked --release --bin plenty --example compile_bench
python3 -B experiments/fixed-state/verify.py --before /path/to/plenty-before
python3 -B experiments/fixed-state/measure.py --before /path/to/plenty-before \
  --references /path/to/nbody-structural-artifacts
PLENTY_CACHE=off perf stat -e instructions:u \
  target/release/examples/compile_bench 1000 10 --workload aot
RUSTC_WRAPPER= just test
```

The references directory can be regenerated using the structural study's
`measure.py --build-only`. Omit `--references` to measure only the compiler
change. Generated executables, dumps and raw perf logs live under
`target/fixed-state/`; the committed JSON records retain the observations.
No runtime ISA dispatch, automatic SIMD packing or general aggregate optimizer
is delivered here. Future scope belongs to the backlog and issue #930.

A CLIF regression additionally checks that all three rows of the small update
kernel become one scalar arithmetic block with no cell accesses or calls.

Full validation passed: `just test` (266 compiler unit tests, 6,541 integration
checks, nine compile-fail checks, 288 runtime checks, and both binary tests;
four integration and five runtime checks ignored); formatting and workspace
clippy. Docs CI passed: 783 Markdown files / 5,708 targets, 367 Python tests
(four skipped), and the complete site (1,590 default / 795 development pages).

## Acknowledgements

- Probes reuse the repository's n-body port and the structural study's C/Rust
  references, under the retained [Benchmarks Game BSD-3-Clause license](../../benchmarks/game/LICENSE-benchmarks-game.txt).
- The existing [CLIF harness and instruction parser](../kernels/README.md) supply
  the structural target and machine-code ledger; no backend dependency is patched.

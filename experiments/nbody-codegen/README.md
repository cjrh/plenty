# n-body and k-nucleotide: instruction diagnosis

Measured at main `55782d72`, 2026-11-09, Ryzen 7 7840HS (x86-64 Linux),
Rust 1.99.1, GCC 16.2.1, Cranelift 0.137.0, Valgrind 3.27.1.
Optimized Plenty builds, cache off; no compiler/runtime changes in this study.
The stored [counts](counts.json) are three `instructions:u` observations per
n-body executable at 1,000,000 steps, including the `taskset` launcher.
The unchecked diagnostic rows used the `--unchecked assume` build mode, which
has since been removed; `run.py` no longer builds them.

The useful result is a separation of costs. Scalar magnitude arithmetic is
already close to C. Packed magnitude arithmetic is supported by Cranelift but
not fully expressible through today's `std.simd` API. Velocity updates retain
checks, spills and address work, and substantial work happens outside the
inner loops. Rust also benefits from fixed-size scalar replacement, unrolling,
and SIMD across the simulation. No measured result here justifies inline
assembly or patching Cranelift.

## Whole-program n-body

| Build | Instructions (millions, median) | Range across three runs |
|---|---:|---:|
| Rust native, reference flags | 229.635 | 914 |
| Rust baseline x86-64 | 451.635 | 133 |
| Rust native, both vectorizers disabled | 395.635 | 664 |
| Rust baseline, both vectorizers disabled | 557.635 | 253 |
| Plenty idiomatic | 2,049.711 | 312 |
| Plenty unchecked diagnostic | 1,198.694 | 323 |
| Plenty clist | 2,584.694 | 77 |

All seven outputs matched within the runner's 1e-8 tolerance. SIMD-disabled
builds use `-C no-vectorize-loops -C no-vectorize-slp`, not fast-math.
The native Rust assembly uses AVX/AVX-512 registers and packed operations;
Plenty's `baseline_isa` disables optional CPU features. The baseline comparison
therefore matters: it separates source/lowering differences from target ISA.
These ablations change instruction selection and scheduling together; their
deltas are not an additive causal decomposition of the whole gap.

Rust's LLVM IR contains `<2 x double>` and `<4 x double>` sqrt, multiply and
divide. Its time-step loop has no remaining pair-loop backedges: the constant
five-body work has been expanded, with much state kept in vector registers.
Plenty's `advance` still walks dynamic lists and scratch buffers. A separate
probe replacing only `n = len(bodies)` with 5 and pair count with 10 runs
2,029.377M checked / 1,182.360M unchecked instructions (one run each, launcher
excluded). That approximately 1% improvement does **not** reproduce Rust's
unrolling or register-resident state. Both outputs matched exactly.

An attempted source expansion of all fixed inner loops reaches Plenty's
256-parameter/local-slot limit before execution. `unroll.py` preserves that
reproduction; its unrolled outputs are intentionally rejected probes, not
replacement benchmark programs. This does not establish a backend unrolling
limit, since a compiler pass could transform IR after frontend slot assignment.

## Where n-body spends instructions

Callgrind at 10,000 steps: `advance` has 20,490,906 self instructions, 98.05%
of the program. Per-instruction counts confirm the following hot paths;
`loops.py` alone also lists fallback paths that do not run for these inputs.

| Inner phase | Static instructions/pass | Dynamic instructions, 10,000 steps |
|---|---:|---:|
| Differences | 34 | 3,320,000 |
| Magnitudes | 18 | 1,800,000 |
| Velocity updates | 64 | 6,320,000 |
| Position updates | 22 | 1,100,000 |
| Everything else in advance | — | 7,950,906 |

Difference and velocity loops run two fewer instructions on their final pass
(the exit bypasses a spill and jump). About 38.8% of `advance` is outside these
four inner bodies, including loop setup, header/length loads, version guards,
outer-loop control, and one-time setup/teardown. This is **not** a claim that
all 38.8% is removable. Dump offsets for this build are `a41..ae1`,
`e15..e63`, `13a1..14ed`, and `178f..17f3` respectively.

## Isolated scalar velocity kernel

[velocity.plenty](velocity.plenty) and [velocity.c](velocity.c) preserve the
pair order, scalar arithmetic, and input types. C gets no `restrict`; it has
ordinary packed C records while Plenty uses its current tagged record layout.
The comparison counts hot inner passes, excluding setup. C's size parameter
is unsigned; Plenty's loop arithmetic and its checks retain signed semantics.

| Build | Instructions/pass |
|---|---:|
| GCC -O2, no fast-math, no restrict | 34 |
| Plenty, body-bounds-proved path | 62 |
| Plenty, fully checked fallback | 70 |
| Plenty, diagnostic unchecked assume | 40 |

Every instruction in the 62-versus-34 hot paths is accounted for below.
This is a machine-code ledger, not a claim each category can be independently
removed without affecting register allocation.

| Category | C | Plenty | Difference |
|---|---:|---:|---:|
| Four scratch-buffer bounds comparisons/branches | 0 | 8 | +8 |
| k overflow reporting (`seto`, `test`, `jne`) | 0 | 3 | +3 |
| Stack spills/reloads | 0 | 6 | +6 |
| Record/scratch address formation | 1 | 6 | +5 |
| Counter updates, comparisons and loop branches | 4 | 6 | +2 |
| FP loads | 8 | 9 | +1 |
| FP stores | 6 | 6 | 0 |
| FP multiply/add/subtract (including memory operands) | 14 | 14 | 0 |
| FP register copies | 1 | 4 | +3 |
| **Total** | **34** | **62** | **+28** |

No negative-index joins, header reloads or calls remain *inside this hot
inner path*. The cached bases and a length still spill; header work remains
outside it. C hoists the first body's mass and advances a body pointer;
Plenty reloads the mass and shifts the second body index by seven for its
128-byte record stride. C also chooses different memory operands for the
arithmetic. The unchecked experiment removes 22 instructions, more than the
11 direct check instructions, because eliminating checks changes pressure,
address selection and loop layout. It is an attribution probe, not a safe fix.

## Magnitude kernel and CLIF variants

[magnitude.plenty](magnitude.plenty) uses normal checked lists. Its loop
version guard selects a no-check path when all four lengths cover `n`; the
fallback preserves the original traps. No checks were removed by this study.

| Build / shape | Instructions/pass | Pairs/pass |
|---|---:|---:|
| GCC -O2 scalar | 17 | 1 |
| Plenty scalar, versioned fast path | 18 | 1 |
| Plenty scalar, checked fallback | 29 | 1 |
| CLIF a: packed pointers, single byte-offset induction | 17 | 1 |
| CLIF b: a with f64x2 loads/arithmetic/store and stride 16 | 16 | 2 |
| GCC -O3 vector loop | 16 | 2 |

GCC flags: `-fno-math-errno -ffp-contract=off`, no `restrict` or fast-math.
`-fno-math-errno` matches Plenty's lack of C errno side effects for sqrt;
C's vector path includes its own alias/version guard and scalar tail.
The CLIF variants are hand-written pointer-kernel shapes with valid byte
lengths as a precondition, not a drop-in replacement for checked lists.
The harness verifies and lowers them but does not execute them.

The scalar Plenty fast path has four loads/stores, eight arithmetic operations,
two register preparation instructions, and four loop-control instructions.
C has the same 14 data/arithmetic/preparation instructions and three loop-control
instructions. Plenty counts both an ascending index and remaining iterations.
Its `xorpd` before `sqrtsd` replaces C's copy before sqrt, so blaming that zeroing
instruction alone would misattribute the one-instruction difference.

The checked Plenty fallback adds eight bounds-check instructions, two spill
accesses and a backedge jump: 18 + 8 + 2 + 1 = 29. The packed CLIF vector
uses `sqrtpd` and `divpd` on baseline SSE2, with no custom assembly. The 16
instructions process two independent distances with unchanged per-lane operation
order. This provides a plausible target of eight instructions per pair before
safe-loading/tail overhead, **not** a measured end-to-end improvement.

`std.simd` currently has no float-vector sqrt. Its list load/store bodies are
scalar, and storage boundaries can undo vector-register promotion. Adding sqrt
alone therefore needs a follow-on measurement of the complete load-compute-store
path before claiming the table's ideal kernel is reachable from public Plenty.

## k-nucleotide: current evidence

Input: fasta 50,000, with 250,000 bases in the third sequence and 1,749,961
counted frames across lengths 1, 2, 3, 4, 6, 12, 18. The current source uses
`get_or_insert`; the old lookup-then-insert explanation is obsolete.

Initial `perf instructions:u`: serial Plenty 1,453,342,160; native Rust
875,153,584. The Rust run includes seven worker threads; counts include their
work, so this is not the misleading serial-versus-parallel wall-time ratio.
Hash seeds vary: these are observations, not a deterministic regression threshold.
Outputs matched byte-for-byte. Native Rust cannot run under local Valgrind
(unsupported opcode), so the Callgrind comparison uses a separately labelled
baseline-x86-64 Rust build: 903,421,622 Ir versus Plenty 1,463,584,957 Ir.

Plenty **self** costs (no inclusive double counting):

| Function | Million Ir | Ir/frame (approximate) |
|---|---:|---:|
| aggregates::hash | 216.5 | 124 |
| get_or_insert native operation (`plenty_op_601`) | 142.3 | 81 |
| count_frames | 107.4 | 61 |
| Collection::probe | 101.6 | 58 |
| Bytes.to_text_range | 94.5 | 54 |
| strings::try_from_utf8 | 93.5 | 53 |
| Collection::locate | 89.2 | 51 |
| Entries::get | 83.7 | 48 |
| aggregates::equal | 51.5 | 29 |
| Collection::try_reserve | 49.9 | 29 |
| std.bytes.text_in | 49.0 | 28 |
| bytes conversion native operation (`plenty_op_183`) | 43.7 | 25 |

Per-frame figures amortize all calls, including growth/other callers, rather
than asserting one call/frame. The emitted `count_frames` loop's non-exit path
has 62 instructions **excluding callees**. Key construction includes ownership,
conversion and error marshalling; the entries/probe layers retain generic
representation work. Rust also spends heavily in hashing and allocation, so
its advantage cannot be inferred from the Plenty hash share alone.

Rust's LLVM/assembly uses hashbrown grouped control-byte scans (`vpmovmskb`,
bit scans, and vector comparisons); Plenty's `aggregates/buckets.rs` probes
one u64 slot at a time, with stored hash bits filtering equality checks.
That is a real design difference, but these measurements do not yet establish
that replacing the table is the best tradeoff. Isolate hit-only dictionary
updates and the string-construction wrapper chain before such a change.

## Reproduce

From the worktree root (all generated outputs stay in `target/`):

```sh
RUSTC_WRAPPER= cargo build --offline --locked --release --bin plenty
RUSTC_WRAPPER= cargo build --offline --locked --release \
  --manifest-path experiments/kernels/harness/Cargo.toml --target-dir target
python3 -B experiments/nbody-codegen/run.py
bash experiments/nbody-codegen/knucleotide.sh
python3 experiments/kernels/loops.py < target/nbody-codegen/magnitude/asm.txt
python3 experiments/kernels/loops.py < target/nbody-codegen/velocity/asm.txt
python3 experiments/kernels/loops.py < target/nbody-codegen/velocity-O2.asm
python3 experiments/kernels/loops.py < target/nbody-codegen/magnitude-clif.txt
python3 -B experiments/nbody-codegen/unroll.py target/nbody-specializations
```

`run.py` uses core 0; the nucleotide script permits cores 0,2,4,6,8,10,12.
Adapt those affinity selections on another machine. `unroll.py` only generates
sources; compile them normally. The fully unrolled variants are expected to hit the slot
limit. The specialized functions assume the benchmark's five bodies and are
not general-purpose APIs.

For full-program attribution:

```sh
PLENTY_CACHE=off target/release/plenty --dump clif-opt,asm --dump-filter advance \
  benchmarks/game/n-body/plenty/nbody.plenty -o target/nbody-codegen/advance
valgrind --tool=callgrind --dump-instr=yes --collect-jumps=yes \
  --callgrind-out-file=target/nbody-codegen/advance.cg \
  target/nbody-codegen/plenty 10000
callgrind_annotate --auto=no target/nbody-codegen/advance.cg
```

## Validation and limits

Both stored Plenty kernels type-check and execute: magnitude prints
`[8e-5, 1e-5]`; velocity prints `-1.0 0.5`. The complete seven-build n-body
comparison was run twice and checked output; the stored JSON is the first run.
The C kernels compiled, and the CLIF variants passed the harness verifier.
The C kernels and CLIF probes were inspected but not linked into a differential
execution test. k-nucleotide outputs matched for the measured builds; the reproduction script
was also rerun successfully. Source-link checks passed (782 Markdown files),
workflow-script tests passed (367 tests, four skipped), and the complete docs
site passed validation (1,588 default and 794 dev HTML pages).

No compiler change was made, so no before/after compile-time count or full
compiler suite is claimed. There is no validated timing improvement yet.
Future implementation must preserve checks/failure order, test behavior and
emission, count compile-time work, and validate runtime timings afterwards.

Findings are recorded in local issues #929 (vector sqrt), #930 (velocity and
surrounding-loop overhead), and #931 (current nucleotide helper costs).
No backlog delivery milestone is claimed by this diagnosis-only change.

## Acknowledgements

The numerical kernels are extracted/adapted from the repository's Benchmarks
Game n-body port and reference by Ilia Schelokov. The reference programs used
here are retained unchanged under the [Benchmarks Game BSD-3-Clause
license](../../benchmarks/game/LICENSE-benchmarks-game.txt); k-nucleotide's
reference was contributed by Renat Galimov. This experiment adds no shipped
runtime code or data.

The [sqrt implementation follow-up](sqrt.md) measures the public SIMD path and
records why the benchmark still uses scalar magnitudes.

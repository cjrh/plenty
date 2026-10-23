# Columnar-list spike evidence

This accompanies the [columnar-list proposal](../columnar-list.md). Sources
and reproduction commands live in `experiments/columnar-list/README.md` in
the repository. These are bounded feasibility experiments, not a production
container, Plenty performance results, or a backend comparison.

## Layout experiment

The dependency-free Rust program compares an array of 64-byte records (eight
`u64` fields), eight flat columns, and groups of 1,024 or 4,096 rows with one
allocation per field. Every layout contains identical generated values.
The kernels sum one field, filter on one field and sum another, mix all eight
fields in sequential row order, and perform the same whole-row work in an
odd-stride permutation. That permutation visits every row once; it is a
repeatable dispersed-access workload, not a model of all random workloads.

All arithmetic deliberately wraps. This is equivalent across the experiment,
but does not model Plenty's default overflow traps. Checked integer and
floating-point kernels need separate end-to-end validation before adoption.
Timed paths allocate nothing; construction is outside the scan measurements.
Append and vacuum are correctness experiments, not performance measurements.

On 2026-10-23: AMD Ryzen 7 7840HS, Linux 7.2.11, rustc 1.99.0
(`b940084d7`), LLVM 23.1.1, native x86-64 target, CPU affinity 4. Per-core
L1d is 32 KiB, L2 is 1 MiB, and shared L3 is 16 MiB. Each layout has
32 KiB, 2 MiB or 64 MiB of row payload at the three tested lengths; the
single selected column has 4 KiB, 256 KiB or 8 MiB. All four layouts coexist.

Release builds use `-C target-cpu=native`; a second build also disables loop
and SLP vectorization. Seven samples rotate layout order; each sample traverses
4,194,304 rows in total. References and outputs cross `black_box`, every result
is checked, and individual scan functions are not inlined into the timed
repetition. These are repeated warm scans with no cache flush. Frequency is
not locked and the machine is shared: load average was approximately 8.1–9.7
on 16 logical CPUs. Treat close rankings and exact ratios as provisional.

### Large input: median nanoseconds per row

1,048,576 rows; lower is better. "Scalar" means both LLVM vectorizers disabled,
not a simulation of Cranelift. Native may use all supported host instructions.

| Build / workload | AoS | Flat SoA | Chunk 1,024 | Chunk 4,096 |
| --- | ---: | ---: | ---: | ---: |
| Scalar: sum one field | 2.463 | 0.251 | 0.446 | 0.334 |
| Scalar: filter two fields | 1.363 | 0.318 | 0.817 | 0.513 |
| Scalar: all fields, sequential | 1.904 | 3.522 | 3.006 | 2.626 |
| Scalar: all fields, permuted | 24.146 | 47.179 | 58.057 | 52.378 |
| Native: sum one field | 4.217 | 0.078 | 0.203 | 0.154 |
| Native: filter two fields | 6.895 | 0.292 | 0.736 | 0.458 |
| Native: all fields, sequential | 10.285 | 2.254 | 2.790 | 2.385 |
| Native: all fields, permuted | 22.904 | 49.426 | 56.298 | 57.228 |

Sample ranges matter: scalar AoS sequential whole-row work ranged
1.707–2.325 ns/row and flat SoA 3.453–4.638. Native permuted work ranged
21.686–27.734 for AoS and 45.205–64.939 for flat SoA. This is enough to
identify tradeoffs, not to choose final tuning parameters.

### Size sensitivity: one-field scan

| Rows | Scalar AoS | Scalar flat SoA | Native AoS | Native flat SoA |
| ---: | ---: | ---: | ---: | ---: |
| 512 | 0.403 | 0.240 | 0.560 | 0.040 |
| 32,768 | 0.810 | 0.256 | 0.735 | 0.064 |
| 1,048,576 | 2.463 | 0.251 | 4.217 | 0.078 |

The large single-column working set fits shared L3 while the complete AoS
does not. Cache residency and reduced traffic are part of the benefit, but
these warm-scan numbers must not be advertised as sustained DRAM bandwidth
or cold-scan speedups. No hardware cache-miss counters were collected.

## Code generation changes the story

Inspection of emitted assembly found `vpgatherqq` in the native AoS narrow,
filter and sequential whole-row kernels. Flat SoA uses contiguous vector
loads/arithmetic (`vpaddq`, and `vprolq` for whole-row mixing), without those
gathers. With both vectorizers disabled, these kernel bodies contain no vector
instructions. Chunk scan functions call the corresponding SoA kernels.

Thus the striking native ratios combine layout, cache residency and a poor
AoS gather strategy on this compiler/CPU. Scalar results still favor columns
for narrow work, but the **sequential whole-row ranking reverses**. Do not
claim that all full-object access is slower in SoA or that LLVM always improves
a loop. The experiment also charges flat SoA's generated indexed row access
its bounds checks and chunked random access its directory/division work.
Future Plenty lowering may optimize those differently.

Flat columns won the measured narrow scans. Fixed chunks did not provide a
scan-speed reason to adopt them here; growth limits, metadata pruning and
deletion behavior need separate measurements. This supports starting flat,
without foreclosing chunked storage later.

## Ownership and metadata experiments

Four Rust tests check:

- Equal row values and scan results at lengths 0, 1, 2, 7, 1,023, 1,024,
  1,025 and 4,097, including chunk tails.
- Failure before either required column reserve: no partial live row, both
  owned fields returned intact, then destroyed exactly once. Real
  `try_reserve` errors share this path. Injection is a deterministic gate,
  not process-wide allocator failure instrumentation.
- Every deletion mask of eight rows: owners destroyed at deletion, repeat
  deletion harmless, stable in-place vacuum preserves row alignment/order,
  repeat vacuum harmless and no duplicate destruction. Safe `Option` slots
  model holes; production bitmap/uninitialized storage is not implemented.
- Conservative min-bound pruning over identical sorted/permuted values:
  a `< 64` query visits 1/8 groups when sorted and 8/8 when dispersed,
  returning the same sum. Statistics are rebuilt for the test, not maintained
  under mutation. This is a correctness demonstration, not a timing result.

These tests reduce implementation risk but leave real runtime allocation
failure, destructor ordering, chunk relocation, and borrowing diagnostics for
the production gate. There is no benchmark of noncopyable/string fields,
concurrent updates, growth, tombstone scan density or memory reclamation.

## Current Plenty feasibility

Using a compiler freshly built from `45798945`, `columns.plenty` creates two
ordinary lists, reserves both before appending, and zips borrowed columns into
a fold. It prints `90`. This is checked language code with ordinary collection
storage and no generated schema support. It establishes that a concrete SoA
wrapper need not wait for clist, not that its elements are packed native words.

`reject-row-reference.plenty` fails with `references cannot be stored in class
fields`. `reject-clist.plenty` fails with `` `clist` is not a generic function ``.
The rejection fixtures document the baseline only, not new defects. The
implementation inspected in `plenty-runtime/src/entries.rs` stores collection
rows in `u128` words; the borrowed-list row helpers in
`plenty-runtime/src/aggregates.rs` expose contiguous row addresses. A columnar
source needs new field/projection lowering and must not masquerade as that ABI.

All four native tests pass, including in release mode; formatting and Clippy
pass for the isolated crate. The Plenty checks are reproducible through
`check_plenty.py`. Existing language/reference behavior is unchanged.

# Generated Plenty column-container spike

This experiment specializes a field-only class into an ordinary Plenty module.
It tests Plenty ownership, native Cranelift code generation, allocation failures,
and performance without adding production `clist` syntax or a compiler intrinsic.
The generated `ClistParticle` owns one private `list[T]` per field. These are
contiguous **16-byte runtime slots**, not tightly packed primitive arrays.

Accepted schema: exactly one public nongeneric class, with public `bool`,
`i8/i16/i32/i64`, `u8/u16/u32/u64`, `f32`, or `f64` fields. All methods,
destructors, custom constructors, reference/owned/nested fields, and empty
schemas are rejected. This is deliberately a narrow generator, not a parser
for arbitrary Plenty source. Source field names and declaration order survive.

The generated operations are `len`, `reserve(additional)`, `append(row)`,
`copy_row(index)`, and `get_FIELD`, `set_FIELD`, `column_FIELD`. Column methods
return ordinary shared list references; bind these to locals before `iter`.
They borrow the entire container, so mutation through its methods is forbidden
until the loan ends. Storage fields are private to the generated module.
There is no fabricated row reference, direct object indexing, field selector,
removal, chunk, tombstone, or packed primitive storage in this experiment.

An append first reserves all columns. A reserve failure returns an error owning
the untouched row; existing rows and all column lengths stay unchanged, while
some capacities may have grown. Only then do scalar appends commit the row.
Their `.unwrap()` relies on successful reservation and nonallocating primitive
copies. This invariant does not extend automatically to arbitrary field types.
Empty construction currently allocates eight list handles, unlike the proposed
allocation-free empty `clist`; failed construction cleans up initialized fields.

From the repository root:

```sh
python3 -B -m unittest discover -s experiments/columnar-list/plenty -p 'test_*.py'
cargo build --locked --bin plenty --features runtime-checks
python3 -B experiments/columnar-list/plenty/check.py target/debug/plenty
# Build without accounting/failure-injection before timing.
cargo build --locked --bin plenty
python3 -B experiments/columnar-list/plenty/bench.py target/debug/plenty \
  --out experiments/columnar-list/out/plenty --cpu 4
```

The compiler's own debug/release build affects compilation time, not the
optimization requested for generated programs: `bench.py` uses `--release`.
Select an allowed CPU or omit pinning. The runner retains generated source,
executables, raw samples and hardware/compiler identity under `--out`.
It checks every checksum against an independent Python calculation and drops
the first warmup sample. Both layouts stay allocated; timed kernels exclude
construction, and layout order alternates between samples. Initial reserved
construction is reported once per layout, not as a robust growth benchmark.

The workload has eight checked `i64` fields. It measures one-field sums,
two-field filter/fold, sequential whole-row reconstruction, and dispersed
whole-row reconstruction using an odd-stride permutation. The whole-row
comparison includes the generated `copy_row` method/Option/checked access
overhead. It is an end-to-end facade comparison, not a pure layout comparison.
An additional `stream` kernel binds all eight column references once and
indexes them directly, separating reconstruction from indexed-access overhead.
No SIMD, LLVM, hardware counters, cache flushing or non-x86 execution is used.

`measurements.json` preserves the 2026-10-24 samples reported in the proposal.
`reject-direct-column.plenty` reproduces issue #427; it is a current limitation,
not a permanent rejection contract. Issue #429 records the indexed-loop
runtime-call cost found in `stream`.

For a standalone generated module:

```sh
python3 -B experiments/columnar-list/plenty/generate.py \
  experiments/columnar-list/plenty/particle.plenty /tmp/columns.plenty
```

Never treat this experiment's generated API as an approved standard library.

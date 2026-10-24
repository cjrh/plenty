# Initial explicit SIMD implementation

This implements the first [`std.simd` module](../../design/stdlib/simd.md),
following the [API proposal](api.md) with deliberate limits. It is separate from
the compiler's packed-column [automatic SIMD](../columnar-list/simd-runtime-bounds.md).

## API and representation decision

Rust's local source checkout at `080cc271463` supplied the model, particularly
`library/portable-simd/crates/core_simd/src/vector.rs`, `masks.rs`, and `ops.rs`:
logical lane values, splats, `simd_*` comparisons and mask selection. Plenty
preserves checked integer arithmetic under `add/sub/mul`; its `wrapping_*`
methods explicitly request Rust-style modular lane arithmetic. No Rust source
was copied into the implementation.

The first types are concrete `i32x4`, `u32x4`, `f32x4`, `f64x2`, `mask32x4`, and
`mask64x2`. They use existing copyable inline enums, so ordinary ownership,
borrowing, aggregates and function boundaries work without new syntax or a new
runtime ABI. Module splat/load factories and the `Lanes` constructor replace
Rust's static methods. Logical mask booleans are public; native mask encoding
is not. Safe list loads/stores assemble ordinary list elements and require a
full vector, rather than promising packed numeric list storage.

Every published operation has an ordinary Plenty scalar body. The optimized
x86-64 path substitutes only trusted embedded methods with exact signatures,
using a closed table of 128-bit operations supported by the baseline backend.
Other hosts retain the scalar bodies. This adds no supported target or runtime
ISA dispatch. Checked integers and ordered float folds remain scalar.

## Keeping local values in registers

A first prototype emitted vector instructions but materialized every result as
four tagged scalar slots. Its integer recurrence took about 172 instructions
per iteration versus 13 for four scalar accumulators, and roughly 28 times the
elapsed time. That was an architectural failure, not an acceptable SIMD result.

The implementation therefore promotes exact numeric-vector locals to private
vector SSA variables. Parameters, generators, masks, and any local that is
mutably borrowed or moved retain canonical storage. Shared borrows materialize
valid snapshots; the ownership checker prevents source mutation during the loan.
Fallback calls and container operations still receive normal enum values.

Cranelift conservatively retains stores to unused stack slots. The lowerer
records its private snapshot instructions, roots snapshots whose SSA results
reach any ordinary instruction, propagates dependencies between snapshots, and
removes only unobserved groups. Calls, returns and block arguments are all uses.
The vector-producing arithmetic is outside these groups. Thus supported local
chains and loops retain vector values without repeatedly scattering lanes.
This is bounded to the explicit module, not a general escape optimizer.

Externally stored four-lane values still have 64-byte tagged payloads plus the
ordinary handle. The public API promises logical lanes and no allocation, not a
packed 16-byte representation, one register, or a foreign vector ABI.

## Evidence and limits

The reproducible [experiments](../../../../experiments/simd/explicit/README.md)
exercise checked results, dependency chains, and throughput on the same host.
They retain unfavorable cases rather than selecting only wins. On the initial
Ryzen 7 7840HS runs, eliminating materialization reduced the integer recurrence
to approximately 14 instructions per iteration. Its SSE2 multiply sequence was
still slower than four scalar accumulators. A single four-lane float recurrence
was close to scalar speed; four independent vectors of divide/add work showed
a useful throughput gain. Retained final timings and counter samples live with
the experiment.

The [integer multiplication assessment](integer-multiplication.md) isolates
baseline instruction synthesis, dependency chains, newer-ISA behavior and an
independent exit-shape spill limitation with the same integer workload.

The module is useful for explicit lane algorithms, but it does not make generic
lists packed or remove all ABI costs. Measure the actual algorithm. Packed
views, wider vectors and more capable target lowering remain separate decisions;
the implementation does not implicitly relax floating-point order to improve a
benchmark.

Differential tests cover integer endpoints and all comparisons, modular and
checked arithmetic, distinct float lanes, NaNs, signed zero, subnormals, mask
selection, full-fit bounds, no partial stores, nested values, copy independence,
mutable projections/captures, shared references, loops/joins and scalar fallback.
Code-generation checks require baseline vector arithmetic while checked integer
operations remain scalar.

# A small portable `std.simd` surface

This records the proposed explicit API for [portable SIMD](../std-simd.md).
The [initial module](../../design/stdlib/simd.md) implements a subset using
existing copyable enums, module factories and named operations; its
[implementation assessment](implementation.md) explains differences from this
sketch. Packed views, static type factories, operator syntax and fully opaque
logical masks below remain proposed.

## Values users can understand

Start with concrete types such as `simd.f32x4`, `simd.f64x2`, `simd.i32x4` and
`simd.u32x4`. Each is an inline fixed-size value with ordinary copy semantics,
not a heap object or a promise of one hardware register. Qualify additional
integer widths only when their operations and scalar fallback are tested.
Keep a closed initial set rather than making users learn const generics or
backend capability traits. Named aliases can survive a later generic design.

Rust's `Simd<T, N>` is a useful model: lane operations, splats, comparisons,
selection and portable load/store behavior. Its full API is not the starting
scope. In particular, do not expose raw pointer vectors, unsafe loads, arbitrary
gather/scatter, dynamic swizzles or architecture-specific intrinsics by default.

| Surface | Proposed contract |
| --- | --- |
| `splat(value)` | Repeat one scalar into all lanes; allocation free |
| Fixed-arity `new(...)`, `lane(index)` | Construct without allocating a list; lane index is checked |
| Floating `+`, `-`, `*`, `/` | Scalar lane semantics, without implicit fusion/reassociation |
| Integer `+`, `-`, `*` | Checked like ordinary Plenty; first implementation may scalarize checks |
| Integer `wrapping_add/sub/mul` | Explicit modular arithmetic, matching existing scalar methods |
| `eq`, `lt`, `le`, etc. | Produce an opaque lane mask, not one boolean |
| `mask.select(yes, no)` | Select already-computed lanes; this is not lazy conditional execution |
| `mask.any()`, `mask.all()`, `mask.count()` | Boolean/count summaries; no exposed integer encoding |
| `load(view, offset)`, `store(view, offset)` | Check a complete vector fits; accept scalar-aligned memory; no overread |
| `wrapping_sum()` on integer vectors | Explicit modular reduction, independent of grouping |
| `sum_ordered(initial)` on float vectors | Scalar left fold in increasing lane order; no speedup promised |

Each comparison mask is compatible only with vectors of the corresponding lane
count/shape; there are no implicit integer-mask casts. Avoid operator-overloading
ambiguity around equality returning a mask: explicit comparison methods suffice.
Mask bit representation, alignment and calling convention are private.

## Keep Plenty's arithmetic contract

Rust SIMD integer operators wrap. Plenty's ordinary integer operators trap on
overflow, so copying Rust's operator behavior would make changing a scalar into
a vector silently change the algorithm. Keep checked operators, expose wrapping
by its existing name, and allow checked vectors to use scalar fallback initially.
For an explicit checked operation, report the lowest failing lane and operation
site. Do not publish partially written stores after a failed bounds check.

No initial integer division, shifts with new count rules, saturating casts,
approximate reciprocal, fused multiply-add, or min/max with underspecified NaN
behavior. Add each only with a complete scalar contract and target tests.
Explicit fused or relaxed operations can be considered later; their names must
make the semantic difference visible.

Floating lanes follow Plenty's scalar rules for signed zero, infinities, NaNs
and subnormals on a supported target. Do not promise bit-identical NaN payloads
across platforms beyond the scalar contract. Rust documents subnormal flushing
on some older architectures; Plenty must either provide an appropriate fallback
or document the target's scalar contract before enabling SIMD there. Do not
silently weaken scalar semantics to accommodate an instruction.

## Bounds, tails and ownership belong below the algorithm

Safe load/store accept the eventual checked contiguous numeric view, retaining
its owner and borrow lifetime. No caller-supplied alignment proof is needed.
Loads do not allocate; stores require an exclusive mutable view and validate
all destination bounds before writing. The initial C ABI uses pointers/lengths
and scalar elements, never a vector passed by value.

For manual kernels, provide a library block-view helper once packed views exist:
conceptually `simd.blocks4(view)` splits a borrow into full four-lane blocks and
a scalar tail. Each block has a safe `load()`/`store()` pair. The helper checks
the split once, does not allocate or create overlapping mutable loans, and
requires the tail to be handled explicitly. This is an ergonomic target, not
an implemented borrowing API. Do not require every user to write alignment
prefixes or masks for ordinary remainders.

Prefer full blocks plus scalar tail over padded masked evaluation initially.
Padding memory safely is insufficient if inactive lanes divide by zero or trap
on a cast. A future masked-load API needs an explicit inactive value and memory
contract; selection alone does not suppress evaluation of either operand.

For everyday algorithms, automatic pipeline lowering performs the split and tail
handling invisibly. Users reach for explicit vectors only when lane relationships
are part of the algorithm. If common examples require repeated mask and tail
bookkeeping, improve the safe helper or the automatic pattern before broadening
the intrinsic catalog.

## Target policy and explanations

Use a documented deployment baseline by default. A native-target build may
require newer CPU features, but that is a build contract, not a different source
API. Runtime dispatch, feature probing and multiple compiled variants remain
separate extensions with code-size and startup costs.

Lack of a SIMD instruction must never make a valid portable program fail to
compile: split or scalarize. A vector IR type's existence is not a support
check. Query a tested operation/type capability table *before* lowering and
exercise its fallback path in tests. A backend compilation error should be a
compiler defect, not the user's mechanism for selecting vector widths.

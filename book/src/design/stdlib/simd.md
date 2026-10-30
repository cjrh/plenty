# `std.simd` — portable explicit lanes

Use this module when an algorithm needs explicit lane operations. Start with
ordinary column pipelines for bulk `clist` queries: their
[automatic SIMD](../facilities/01-builtins/clists.md#automatic-simd-counts)
handles vector widths and tails without changing the source algorithm.

## Values and construction

`i32x4`, `u32x4`, `f32x4`, and `f64x2` are nominal, copyable values. Their names
describe scalar types and logical lane counts, not an instruction set. Construct
them with `i32x4.Lanes(a, b, c, d)` or the matching module factory, for example
`splat_i32x4(value)`. Construction, arithmetic, comparison and selection allocate
nothing. Values can be passed, returned, nested in other values and stored in
collections under the ordinary enum rules.

`lane(index)` returns `Option[Scalar]`; indices outside `0..lane_count` return
`Nothing`, including negative indices. `to_tuple()` copies the logical lanes in
order, without allocation. Ordinary `==` compares complete values; use the
`simd_*` methods below when each lane needs its own comparison result.

Construct distinct lanes, then add the same value to each with explicit wrapping:

```plenty
import std.simd as simd
def main() -> Result[(), IoError]:
    values = simd.u32x4.Lanes(0u32, 1u32, 2u32, u32.MAX)
    print(values.wrapping_add(simd.splat_u32x4(1u32)).to_tuple())?
    Ok(())
```
```output
(1, 2, 3, 0)
```

## Arithmetic and masks

| Operation | Contract |
| --- | --- |
| `add`, `sub`, `mul` | Scalar lane arithmetic; integer overflow traps |
| Float `div` | Scalar IEEE division, including infinities and NaNs |
| Integer `wrapping_add`, `wrapping_sub`, `wrapping_mul` | Modular lane arithmetic |
| Integer `wrapping_sum()` | Modular sum of all lanes |
| Float `sum_ordered(initial)` | Left fold from `initial`, in increasing lane order |
| `simd_eq/ne/lt/le/gt/ge(other)` | Lane comparisons returning a compatible mask |

Integer checked operations execute in lane order and retain ordinary overflow
diagnostics at the call site. Floating operations do not implicitly fuse or
reassociate. NaN comparisons, signed zero and subnormals follow the target's
scalar rules; NaN payload identity is not promised.

Comparisons on 32-bit lanes produce `mask32x4`; `f64x2` produces `mask64x2`.
Masks contain logical booleans, constructed directly with `Lanes(...)` or by
comparison. There are no integer-mask conversions or public native bit encoding.

`mask.select(yes, no)` selects each lane from two vectors of the same type and
the matching mask shape. It evaluates both operands before selection: it does
not suppress side effects, allocation failures or traps while computing them.
`any()`, `all()` and `count()` summarize a mask. `invert()`, `and_mask(other)`,
`or_mask(other)` and `xor_mask(other)` combine logical lanes.

Select a floor for the lanes below it; the input stays unchanged:

```plenty
import std.simd as simd
def main() -> Result[(), IoError]:
    values = simd.f32x4.Lanes(18.0f32, 21.0f32, 16.0f32, 24.0f32)
    floor = simd.splat_f32x4(20.0f32)
    below = values.simd_lt(floor)
    print(below.count(), below.select(floor, values).to_tuple())?
    Ok(())
```
```output
2 (20.0, 21.0, 20.0, 24.0)
```

Like other generic Plenty helpers, `select[T]` forwards structurally to
`yes.selected(mask, no)`. The module's vector implementations of that helper
are private. A user-defined type may supply its own public `selected` method;
its effects and allocation behavior then belong to that implementation, and
the compiler does not replace it with a vector intrinsic.

## Safe list access

`load_i32x4(values, offset=0)` and the corresponding factories for other vector
types accept a shared list borrow and return `Option[Vector]`. They return
`Nothing` unless a whole vector fits. `vector.store(&mut values, offset)` returns
`True` after replacing all lanes, or `False` without changing the list if the
whole vector does not fit. Negative offsets are invalid. Bounds are checked
before adding lane offsets, including for `i64.MAX`.

These operations neither allocate nor grow the list. They access ordinary list
elements individually; lists are not packed native arrays. Returned vectors own
copies of their scalar lanes, so no borrow persists after loading. Stores take
an exclusive borrow and preserve length and capacity. Process full groups and
then the scalar tail explicitly when writing a manual kernel.

Handle a load that may not fit. A failed store leaves every element unchanged:

```plenty
import std.simd as simd
def main() -> Result[(), AllocError | IoError]:
    mut values = [1i32, 2i32, 3i32, 4i32]?
    match simd.load_i32x4(values):
        case Some(lanes):
            print(lanes.store(&mut values, 1))?
            print(lanes.add(simd.splat_i32x4(10i32)).store(&mut values, 0))?
        case Nothing:
            print("A full vector does not fit")?
    print(values)?
    Ok(())
```
```output
False
True
[11, 12, 13, 14]
```

## Rust relationship and initial limits

The model follows Rust's portable `std::simd`: fixed logical lanes, splats,
per-lane arithmetic and `simd_*` comparisons, masks, and selection. This first
Plenty module has concrete types rather than `Simd[T, N]` and uses methods rather
than overloaded arithmetic operators. Enum factories use module functions and
the `Lanes` variant because Plenty has no static enum methods. Unlike Rust's
wrapping integer operators, `add/sub/mul` preserve Plenty's checked arithmetic;
modular operations have explicit `wrapping_*` names.

The implementation uses existing inline enums, whose payload stores each lane
in a tagged scalar slot. A four-lane payload currently occupies 64 bytes, plus
the ordinary value handle; this is **not** a packed 16-byte vector or a native
register ABI. Mask variants expose logical booleans, not hardware masks. Neither
vector nor mask layout is a C, serialization, Arrow or Parquet contract.

On x86-64, `--release` lowers wrapping arithmetic, float arithmetic, comparisons
and selection to baseline 128-bit SIMD. Integer `wrapping_sum` reduces lanes
directly without calling the scalar module body. Numeric vector locals without mutable
borrows stay in vector registers across supported operations and loops. Values
materialize in ordinary tagged storage at calls, containers and observable
borrows; parameters, masks, generators and mutable-borrowed locals retain that
storage path. This is an internal optimization, not a different ownership rule.
Checked integer arithmetic, mask summaries, ordered float reductions and list access keep
their ordinary scalar implementations. On other compiler hosts, the module's
scalar bodies remain available. The AArch64 implementation currently uses these
scalar bodies, including in release builds; ARM vector acceleration is separate
work.
The same source semantics apply with and without optimization. No runtime ISA
selection or newer CPU feature is required.

Storage boundaries, copying and lane assembly can dominate short operations.
Explicit SIMD is not a blanket speedup over scalar code, and this module does
not replace the packed-column lowering used by `clist`. Wider vectors, packed views, generic
lane counts, gathers/scatters, casts, shifts and integer division are not in this
initial surface. See the [tutorial](../../tutorial/07-modules/19-portable-simd.md)
and the [implementation evidence](../../proposals/std-simd/implementation.md).

## Acknowledgements

- Design follows Rust's portable [`std::simd`](https://doc.rust-lang.org/std/simd/index.html).

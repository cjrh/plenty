# `std.simd` — portable explicit lanes

Use this module when an algorithm needs explicit lane operations. Start with
ordinary column pipelines for bulk `clist` queries: their
[automatic SIMD](../facilities/01-builtins/clists.md#automatic-simd-counts)
handles vector widths and tails without changing the source algorithm.

## Values and construction

`i32x4`, `u32x4`, `f32x4`, `f64x2` and `f64x4` are nominal, copyable values. Their names
describe scalar types and logical lane counts, not an instruction set. Construct
them with `i32x4.Lanes(a, b, c, d)` or the matching module factory, for example
`splat_i32x4(value)`. Construction, arithmetic, comparison and selection allocate
nothing. Values can be passed, returned, nested in other values and stored in
collections under the ordinary enum rules.

`lane(index)` returns `Option[Scalar]`; indices outside `0..lane_count` return
`Nothing`, including negative indices. `to_tuple()` copies the logical lanes in
order, without allocation. Ordinary `==` compares complete values; use the
`simd_*` methods below when each lane needs its own comparison result.

`f64x4.low()` and `f64x4.high()` return lanes 0-1 and 2-3 as `f64x2` values;
`low.concat(high)` joins two `f64x2` values into an `f64x4`. All three copy
lanes and allocate nothing.

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
| `add`, `sub`, `mul` | Integer vectors return `Result[Vector, OverflowError[Lane]]`; float vectors stay infallible |
| Float `sqrt()` | Per-lane scalar square root: negative inputs yield NaN, signed zero and positive infinity are preserved |
| Float `div` | Scalar IEEE division, including infinities and NaNs |
| `f64x2.to_f32x4()` | Lanes 0 and 1 are the two lanes cast with `f32(lane)`; lanes 2 and 3 are `0.0` |
| `f32x4.to_f64x2()` | Lanes 0 and 1 cast with `f64(lane)`, which is exact; lanes 2 and 3 are not read |
| `f64x4.to_f32x4()` | Every lane cast with `f32(lane)` |
| `f32x4.to_f64x4()` | Every lane cast with `f64(lane)`, which is exact |
| Integer `wrapping_add`, `wrapping_sub`, `wrapping_mul` | Modular lane arithmetic |
| Integer `wrapping_sum()` | Modular sum of all lanes |
| Float `sum_ordered(initial)` | Left fold from `initial`, in increasing lane order |
| `simd_eq/ne/lt/le/gt/ge(other)` | Lane comparisons returning a compatible mask |

Integer checked operations execute in lane order and return the first overflowing
lane’s scalar `OverflowError`, including its wrapped value. Callers handle this
ordinary `Result`; an `unchecked:` caller does not change the method’s contract. Floating operations do not implicitly fuse or
reassociate. NaN comparisons, signed zero and subnormals follow the target's
scalar rules; NaN payload identity is not promised.

The integer reduction API is explicitly `wrapping_sum()`: it associates as
`(((lane0 + lane1) + lane2) + lane3)`, wrapping each addition. There is no
ordinary fallible integer reduction method. Float `sum_ordered(initial)` also
folds left in lane order, without reassociation.

Comparisons on 32-bit lanes produce `mask32x4`; `f64x2` produces `mask64x2`
and `f64x4` produces `mask64x4`.
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

These operations neither allocate nor grow the list. The list stays an ordinary
`list` of scalars: no alignment is exposed. Returned vectors own
copies of their scalar lanes, so no borrow persists after loading. Stores take
an exclusive borrow and preserve length and capacity. For a loop over whole lists, prefer
[indexed access through views](#indexed-access-through-views), which tests each
list's length once.

Handle a load that may not fit. A failed store leaves every element unchanged:

```plenty
import std.simd as simd
def main() -> Result[(), AllocError | IoError | OverflowError[i32]]:
    mut values = [1i32, 2i32, 3i32, 4i32]?
    match simd.load_i32x4(values):
        case Some(lanes):
            print(lanes.store(&mut values, 1))?
            print(lanes.add(simd.splat_i32x4(10i32))?.store(&mut values, 0))?
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

## Indexed access through views

Use these in a loop over lists. A [slice view](../language/04-expressions/slices.md),
`&[Lane]` or `&mut [Lane]`, is the packed view: it borrows the list for as long
as it is used, so the list cannot be resized meanwhile, and it allocates
nothing. `&values`, a `&list[Lane]` reference and a slice such as
`&values[1:]` all convert to one.

| Operation | Contract |
| --- | --- |
| `len_f64x2(values: &[f64]) -> i64` | Whole vectors in the view: `len(values) // 2` |
| `tail_f64x2(values: &[f64]) -> i64` | Index of the first element no vector covers: `len_f64x2(values) * 2` |
| `get_f64x2(values: &[f64], index: i64) -> f64x2` | Vector `index`: elements `index * 2` and `index * 2 + 1` |
| `vector.set(values: &mut [f64], index: i64) -> ()` | Replaces the elements of vector `index` |

`f64x4` has the same four operations over `f64` views, four elements to a
vector; `f32x4`, `i32x4` and `u32x4` have them over `f32`, `i32` and `u32`
views, also four elements to a vector. One naming rule covers the module:
an operation with a vector operand is a method on it (`set`, like `store`); one
without is a module function ending in the vector type (`get_f64x2`, like
`load_f64x2`).

Vector `index` always starts at element `index * lanes` of the view. The
accesses cover whole vectors only: when the length is not a multiple of the
lane count, the last `len(values) % lanes` elements belong to no vector and no
access reads or writes them. Process them as scalars with
`range(simd.tail_f64x2(values), len(values))`.

An index outside `0..len_f64x2(values)`, negative included, is a
[trap](conventions.md#invalid-arguments), as an out-of-range list index is. The
trap report names the operation and the calling line:

```text
error: std/simd.plenty:673:13: std.simd.get_f64x2: index is outside 0..len_f64x2(values)
note: called from main.plenty:5:11
```

The index is a vector position, where `load_*` and `store` take an element
offset and report a span that does not fit as `Nothing` or `False`. Keep those
for a single access at an arbitrary offset.

Scale a list in place, then finish the two elements no vector covers:

```plenty
import std.simd as simd

def scale(values: &mut [f32], factor: f32) -> ():
    by = simd.splat_f32x4(factor)
    for i in range(simd.len_f32x4(values)):
        simd.get_f32x4(values, i).mul(by).set(values, i)
    for i in range(simd.tail_f32x4(values), len(values)):
        values[i] = values[i] * factor

def main() -> Result[(), AllocError | IoError]:
    mut values = [1.0f32, 2.0f32, 3.0f32, 4.0f32, 5.0f32, 6.0f32]?
    scale(&mut values, 0.5f32)
    print(values)?
    print(simd.len_f32x4(&values), simd.tail_f32x4(&values))?
    print(simd.get_f32x4(&values[2:], 0).to_tuple())?
    Ok(())
```
```output
[0.5, 1.0, 1.5, 2.0, 2.5, 3.0]
1 4
(1.5, 2.0, 2.5, 3.0)
```

Plenty cannot yet keep a reference in a class field (backlog B11), so there is
no separate view class with a `len()` method: the builtin slice view carries
the borrow and `len_<vector>` counts its vectors.

## Rust relationship and initial limits

The model follows Rust's portable `std::simd`: fixed logical lanes, splats,
per-lane arithmetic and `simd_*` comparisons, masks, and selection. This first
Plenty module has concrete types rather than `Simd[T, N]` and uses methods rather
than overloaded arithmetic operators. Enum factories use module functions and
the `Lanes` variant because Plenty has no static enum methods. Unlike Rust's
wrapping integer operators, `add/sub/mul` preserve Plenty's checked arithmetic;
modular operations have explicit `wrapping_*` names.

The implementation uses existing inline enums, whose payload stores each lane
in a tagged scalar slot. A four-lane payload, `f64x4` included, currently occupies 64 bytes, plus
the ordinary value handle; this is **not** a packed 16-byte vector or a native
register ABI. Mask variants expose logical booleans, not hardware masks. Neither
vector nor mask layout is a C, serialization, Arrow or Parquet contract.

On x86-64, the default optimized profile lowers wrapping arithmetic, float arithmetic (including `sqrt`), comparisons
and selection to baseline 128-bit SIMD. The two-lane conversions lower to `cvtpd2ps` and
`cvtps2pd`. Numeric `Lanes` constructors and
`to_tuple()` followed by unpacking preserve native lanes without a scalar call
or a round trip through tuple storage. Unborrowed matching numeric tuple locals
used by unpacking can also stay native; escaping tuples retain their ordinary ABI.
Integer `wrapping_sum` reduces lanes
directly without calling the scalar module body. Numeric vector locals without mutable
borrows stay in vector registers across supported operations and loops. Values
materialize in ordinary tagged storage at calls, containers and observable
borrows; parameters, masks, generators and mutable-borrowed locals retain that
storage path. This is an internal optimization, not a different ownership rule.
List loads and stores read or write all lanes as one unaligned packed vector
after the same bounds test as the scalar bodies; each call reads the list's
length and performs that test, and a span that does not fit still yields
`Nothing` or `False` without a trap. An unborrowed `Option[Vector]` local,
`match` and `unwrap()` keep a loaded vector native. `get_*` and `set` test the
index against the view's vector count with one comparison, then read or write
one unaligned packed vector. In a `range` loop whose counter is the index and
whose views were bound before the loop (view parameters, or
`xs: &[f64] = &values`), the counts are tested once before the loop and the
loop body keeps no test; the original loop remains as the fallback and reports
the trap. Converting a list to a view inside the loop keeps the comparison on
each access. `len_*`, `tail_*` and `splat_*` lower to inline arithmetic.
Checked integer arithmetic,
mask summaries and ordered float reductions keep
their ordinary scalar implementations. On other compiler hosts, the module's
scalar bodies remain available. The AArch64 implementation currently uses these
scalar bodies, including in optimized builds; ARM vector acceleration is separate
work.
The same source semantics apply with and without optimization. No runtime ISA
selection is performed. At
[`--cpu x86-64-v3`](../toolchain/01-building/linking.md#cpu-levels) the 128-bit
operations use VEX encodings, nothing is fused into FMA, and results are
bit-identical to the default level and to the unoptimized scalar bodies,
including NaN payloads, signed zeros and infinities.

Below x86-64-v3, `f64x4` lowers to two 128-bit `f64x2` halves: each operation
is two native operations, a load or store is two unaligned 16-byte accesses
after one bounds test, and `low()`, `high()` and `concat` select or pair halves
without moving lanes. A `mask64x4` is likewise two native 128-bit masks. `f64x4.to_f32x4()` narrows
each half and joins them with one lane move; `f32x4.to_f64x4()` widens lanes
0-1, then moves lanes 2-3 down and widens them. `f64x4` costs what the same work written as two `f64x2` values costs.

At x86-64-v3 on Linux and macOS, an `f64x4` or `mask64x4` is one 256-bit `ymm`
register: arithmetic, `sqrt`, comparisons, `select` and bitwise mask operations
are one AVX instruction, a load or store is one unaligned 32-byte access after
one bounds test, `low()` and `high()` extract a 128-bit half and `concat`
inserts one; the float lane conversions convert the two halves. Only a function that performs an `f64x4` operation holds such a
value; it spills the value across calls and clears the upper halves before each
call and return, and if the backend cannot lower it, the function is compiled
with two halves instead (see
[CPU levels](../toolchain/01-building/linking.md#cpu-levels)); all of its float
spills and moves are 256 bits. On AMD Zen 4, 256-bit division and square root
take half the time per lane of their 128-bit forms; on Intel cores whose
dividers are 128 bits wide only addition and multiplication are expected to
gain ([measurements](../../completed-work/performance.md#f64x4-in-ymm-registers-issue-962-stage-4)).
Windows keeps the two halves at every level. Lane counts that are not a multiple
of the vector width still need a scalar or narrower tail, and that tail can cost
more than the vector work it follows.

Storage boundaries, copying and lane assembly can dominate short operations.
Explicit SIMD is not a blanket speedup over scalar code, and this module does
not replace the packed-column lowering used by `clist`. `f32x8`, integer vectors wider than 128 bits, fused multiply-add, a view type that owns its vector count, generic
lane counts, gathers/scatters, casts other than the float lane conversions, shifts and integer division are not in this
initial surface. See the [tutorial](../../tutorial/07-modules/19-portable-simd.md)
and the [implementation evidence](../../proposals/std-simd/implementation.md).

## Acknowledgements

- Design follows Rust's portable [`std::simd`](https://doc.rust-lang.org/std/simd/index.html).

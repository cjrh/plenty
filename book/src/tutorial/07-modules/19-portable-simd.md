# Work with explicit SIMD lanes

Prefer ordinary pipelines for repeated column scans: the
[columnar-list lessons](../special-purpose/columnar-lists/index.md) show how
Plenty can accelerate them automatically. Use `std.simd` when the algorithm
itself is clearer as a fixed set of lanes.

This example compares four measurements with one threshold, then selects the
threshold for lanes below it. The mask is a vector of logical booleans; `count`
counts its true lanes, and `select` chooses between already-computed values.

```plenty
import std.simd as simd

def main() -> Result[(), IoError]:
    readings = simd.f32x4.Lanes(18.0f32, 21.0f32, 16.0f32, 24.0f32)
    floor = simd.splat_f32x4(20.0f32)
    below = readings.simd_lt(floor)
    adjusted = below.select(floor, readings)
    print(below.count(), adjusted.to_tuple())?
    print(readings.to_tuple())?
    Ok(())
```
```output
2 (20.0, 21.0, 20.0, 24.0)
(18.0, 21.0, 16.0, 24.0)
```

Vectors are copyable and allocate nothing. Operations borrow their inputs and
produce new values, so the original readings remain unchanged. `i32x4`, `u32x4`
and `f64x2` provide the same pattern with other lane types. Mask shapes must
match the vectors they select.

Use `add`, `sub` and `mul` for ordinary arithmetic. Integer versions return a
`Result` with a lane-typed overflow error; propagate it with `?` or handle it
with `match`. Integer `wrapping_add`, `wrapping_sub` and `wrapping_mul` explicitly
choose modular arithmetic. Floating `sum_ordered(initial)` preserves scalar
addition order. Selection is eager: compute and handle both inputs before
selecting between them.

## Loop over lists

To process whole lists, pass them as [slice views](../04-collections/slices.md)
and index them by vector. `len_f64x2(xs)` counts the whole vectors in a view,
`get_f64x2(xs, i)` reads vector `i` (elements `2 * i` and `2 * i + 1`) and
`vector.set(out, i)` writes one. Five elements hold two whole vectors; no
vector access touches the fifth. `tail_f64x2(xs)` is the index where those
leftover elements start, so a scalar loop finishes them:

```plenty
import std.simd as simd

def distances(xs: &[f64], ys: &[f64], out: &mut [f64]) -> ():
    for i in range(simd.len_f64x2(xs)):
        x = simd.get_f64x2(xs, i)
        y = simd.get_f64x2(ys, i)
        x.mul(x).add(y.mul(y)).sqrt().set(out, i)
    for i in range(simd.tail_f64x2(xs), len(xs)):
        out[i] = (xs[i] * xs[i] + ys[i] * ys[i]).sqrt()

def main() -> Result[(), AllocError | IoError]:
    xs = [3.0, 6.0, 5.0, 8.0, 0.0]?
    ys = [4.0, 8.0, 12.0, 15.0, 2.0]?
    mut out = [0.0, 0.0, 0.0, 0.0, 0.0]?
    distances(&xs, &ys, &mut out)
    print(out)?
    Ok(())
```
```output
[5.0, 10.0, 13.0, 17.0, 2.0]
```

A vector index outside `0..len_f64x2(view)` traps, like a list index out of
range: here `ys` and `out` must be at least as long as `xs`. A view borrows its
list, so the compiler rejects resizing the list while the view is still used.
The other vector types follow the same names: `len_i32x4`, `tail_i32x4`,
`get_i32x4` and `set` on an `i32x4`.

Taking view parameters, as `distances` does, lets the optimized x86-64 build
test each view's length once before the loop instead of on every access.

For one access at an arbitrary element offset, `load_f64x2(values, offset)`
returns `Option[f64x2]` and `vector.store(&mut values, offset)` returns `bool`;
they report a vector that does not fit instead of trapping.

Vector values stored in collections or passed to functions use tagged scalar
storage, so explicit lanes do not promise faster code. The default optimized
profile enables supported native SIMD operations on x86-64, including packed
list access. AArch64 currently uses the scalar implementation, which preserves
the same results. See the [reference](../../design/stdlib/simd.md) for the exact
operations, bounds contracts and current limits.

Float vectors also support `sqrt()` without allocation or a `Result`. Each lane
follows scalar float square root, including NaNs for negative inputs:

```plenty
import std.simd as simd
def main() -> Result[(), IoError]:
    values = simd.f64x2.Lanes(4.0, 9.0)
    print(values.sqrt().to_tuple())?
    Ok(())
```
```output
(2.0, 3.0)
```

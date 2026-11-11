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

The module also provides checked list loads and stores; they require a complete
vector to fit and never allocate. Handle any leftover elements with a scalar
loop. Vector values stored in collections or passed to functions use tagged
scalar storage, and every load and store checks its bounds, so explicit lanes do
not promise faster code. The default optimized profile
enables supported native SIMD operations on x86-64, including packed list loads
and stores. AArch64 currently uses the
scalar implementation, which preserves the same results. See the [reference](../../design/stdlib/simd.md)
for the exact operations, bounds contracts and current limits.

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

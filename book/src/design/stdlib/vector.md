# std.vector

`from std.vector import Vec3` provides a copyable class with three public `f64`
fields, `x`, `y` and `z`. Its scalar Plenty implementation defines the results;
optimized builds may keep eligible local values in private SIMD registers.
The initial implementation is a bounded source-level prototype for
[declared vectors](../../proposals/declared-vectors.md), with ordinary class
storage and call boundaries. It does not promise compact packed storage.

```plenty
from std.vector import Vec3

def main() -> ():
    position = Vec3(1.0, 2.0, 3.0)
    velocity = Vec3(0.5, -1.0, 2.0)
    moved = position + velocity * 2.0
    print(moved.to_tuple(), position.dot(velocity)).unwrap()
```
```output
(2.0, 0.0, 7.0) 4.5
```

## Operations

All operations borrow vector operands, allocate nothing and are infallible.
The generated constructor `Vec3(x: f64, y: f64, z: f64)` copies its components.

| Operation | Result | Meaning |
|---|---|---|
| `a + b`, `a - b` | `Vec3` | Componentwise addition or subtraction |
| `-a` | `Vec3` | Componentwise negation |
| `a * scale`, `scale * a` | `Vec3` | Multiply each component by an `f64` |
| `a.mul(b: &Vec3)` | `Vec3` | Componentwise multiplication |
| `a.dot(b: &Vec3)` | `f64` | `(a.x * b.x + a.y * b.y) + a.z * b.z` |
| `a.length()` | `f64` | `a.dot(a).sqrt()` |
| `a.lane(index: i64)` | `Option[f64]` | Components 0, 1, 2; `Nothing` for every other index |
| `a.to_tuple()` | `(f64, f64, f64)` | Copy the three components |

`lane` deliberately follows the approved checked-vector API rather than
trapping on an invalid position. Negative indices do not wrap. `length` uses
the displayed formula, which can overflow or underflow; it is not a scaled
hypotenuse algorithm. Equality and formatting use ordinary class rules.

Arithmetic preserves scalar component results, ordered dot-product addition,
signed zeros, infinities and subnormals. It does not fuse multiply-adds or use
approximate reciprocal square roots. NaN payload bits and trapping exception
order are not portable guarantees. A private fourth register lane is never a
component and never reads beyond the three stored fields. Its operations use
neutral inactive operands and restore positive-zero padding.

## Ownership and current costs

Assignments, returns, generic calls and collection insertion have ordinary
copy-class semantics. A mutable field can be borrowed using `&mut value.x`;
normal loan checks prevent conflicting access. Such addressable values keep
ordinary storage instead of native-local promotion.

The current private ABI reserves 16 bytes per field: a `Vec3` record occupies
48 bytes, with a 16-byte owning value slot where that slot is needed. Nested
class fields embed the 48-byte record; this does not implement the proposed
32-byte packed representation. Lists and inline arrays accept `Vec3` through
their existing class layout. `clist[Vec3]` uses the existing three scalar columns; nested vector-field
column flattening is not implemented. There is no new C ABI or raw-layout guarantee.

Native promotion is an optimization in eligible optimized x86 functions.
The default CPU level uses two 128-bit parts; `x86-64-v3` can use a 256-bit
part. Fast compilation, other targets and unsupported backend operations use
the scalar body or the existing narrow fallback. There is no runtime ISA
selection beyond the executable's existing requested-CPU check.

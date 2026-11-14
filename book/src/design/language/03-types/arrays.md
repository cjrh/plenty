# Fixed inline arrays

`array[T, N]` holds exactly `N` values of one type. `N` is a nonnegative
`i64` constant or [value parameter](../07-generics/values.md). The array has
no heap header and construction allocates no array storage. Each distinct
length is a distinct type. An array copies when its elements copy; otherwise
it moves. Equality and printing examine elements in index order.

## Complete construction

A nonempty list-shaped literal becomes an array when an annotation, parameter,
constructor field, or declared return supplies an array type:

```plenty
def block() -> array[u8, 3]:
    [1, 2, 255]

def main() -> Result[(), IoError]:
    xs = block()
    print(xs, len(xs), xs[-1])?
    Ok(())
```
```output
array[1, 2, 255] 3 255
```

The count must match the expected length. Elements evaluate once, from left
to right, with the element type as context. The literal itself needs no `?`;
element expressions may still fail. Failure cleans up the initialized prefix.
Without array context, a list-shaped literal retains its ordinary fallible
list meaning. Empty `[]` and comprehensions are not array literals.

`array[T, N].filled(value)` evaluates `value` once and copies it into each
position. It requires an ordinarily copyable element type, including when
`N` is zero. Use `array[T, N].from_fn(factory)` for a reusable callback
`factory(i: i64) -> T`; it is called once for each index from zero through
`N - 1`. `array[T, N].try_from_fn(factory)` accepts a callback returning
`Result[T, E]` and returns `Result[array[T, N], E]`. Its first error stops
construction and destroys the initialized prefix. Neither factory exposes a
partly initialized array. A zero-length factory is never called, although its
callback expression is still evaluated.

At a generic parameter declared as `array[T, N]`, a literal supplies its count
as evidence for `N` and its element type as evidence for `T`. This shape
propagates through references, tuples, and nested array elements. Unsuffixed
integer elements default to `i64` when `T` is unknown. Bare type parameters
and other nominal wrappers do not supply array shape. See the
[accepted literal design](../../../proposals/value-generics.md#literals-at-generic-parameters).

## Access and ownership

`len(a)` is the fixed count. `a[i]` accepts a runtime `i64` index, with negative
indices counting from the end. Out-of-range indices trap, including every
index of a zero-length array. A read copies a copyable element; owned elements
are observed or explicitly copied through a borrow. Indexed assignment
replaces an element of a mutable array. `&a[i]` and `&mut a[i]` use ordinary
owner and element loan rules.

Iterate with `for item in &a` or `for item in &mut a`; each item is a reference.
`reversed(&a)` traverses the same elements backward. Borrowed iteration keeps
the source borrowed across the loop, including its backedge. Moving an array
into an iteration or collection conversion is currently unsupported.

## Storage and limits

References, views, generators, and other types rejected by ordinary aggregate
storage cannot be array elements, even at length zero. An array preserves
its element type's copy and ownership restrictions at length zero. Arrays do
not provide arithmetic, slicing, resizing, or SIMD operations.

Primitive elements use their native byte widths. Nested arrays use their
exact logical payload width as their element stride: `array[array[u8, 3], 2]`
has six payload bytes, with no padding between its three-byte elements.
Ordinary class and enum element representations retain their existing field
storage. The outer value's ABI reservation may include tail padding to a
16-byte boundary and an ordinary value slot; these are separate from the
array's logical payload. For example, `array[u8, 3]` has three payload bytes
and reserves 16 payload bytes in a standalone ABI slot; `array[i64, 3]` has
24 payload bytes and reserves 32. A zero-length array has zero payload bytes.

Layouts are checked before code generation, including inner types under a
zero-length outer array. The existing 65,536-byte inline-payload limit bounds
large instances. Prefer heap collections for large or dynamically sized data.

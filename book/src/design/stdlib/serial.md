# std.serial

`std.serial` defines a JSON-shaped, format-neutral codec boundary. The compiler
specializes `encode[T, E, S](value: &T, out: &mut S)` and
`decode[T, E, D](source: &mut D)` into ordinary checked Plenty calls. Both
return `Result[..., SerialError[E]]`; `S` satisfies `Encoder[E]` and `D`
satisfies `Decoder[E]`. Format algorithms are Plenty code.

Use the [JSON typed entry points](json-codecs.md) for complete documents or
direct conversion to and from `Json`. There is currently no `@serial`
derivation, TOML adapter, reflection, or user macro requirement. The
[serialization proposal](../../proposals/derived-serialization.md) records the
staged design and rationale.

## Built-in representations

| Plenty type | Representation |
|---|---|
| `bool`, `str` | Boolean, text |
| Signed and unsigned integers | Integer, with checked decoding into the requested width |
| `f32`, `f64` | Decimal; each format controls finite-number support |
| `list[T]`, `set[T]` | Array; duplicate set elements are rejected |
| `dict[str, T]` | Object; duplicate keys are rejected |
| Tuple | Fixed-length array |
| `Option[T]` | `Nothing` is null; `Some` uses `T`'s representation |
| `Box[T]` | `T`'s representation |
| Class or enum | Handwritten hooks |

Nested `Option[Option[T]]`, including through transparent `Box` wrappers, is
rejected because two levels of `Nothing` would
both encode as null. Bytes, `Result`, unions, dictionary keys other than
strings, and classes or enums without hooks have no automatic representation.
Collection ordering follows the collection's iteration order.

## Handwritten hooks

Declare private methods in the type's own module with these fixed shapes
(replace `Owner` with the class or enum name):

```text
def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), std.serial.SerialError[E]]
def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[Owner, std.serial.SerialError[E]]
```

The encode receiver is shared; decoding has no `self` receiver. Only
`std.serial.encode` and `std.serial.decode` dispatch these hooks. They may
cross module privacy boundaries just as automatic destruction does. They
never expose private fields or bypass a constructor: the decode hook must
construct the value through the type's normal validated constructor.

For an object, encode `start_object`, each `key` followed by one encoded
value, then `end_object`. Decode `start_object`, call `next_key` until
`Nothing`, and decode one value for each returned key. Keep an `Option` slot
per field to detect duplicates and missing fields. Supply intentional defaults
after reading the object and before construction. Call `unknown_key` for
unrecognized fields so the adapter's reject/ignore policy applies. A missing
field and an explicitly encoded null are different cases.

`wrap(result)` lifts a format error into a `SerialError`. `at(result, step)`
adds context only when the operation failed. Use `Step.Field(name)` for a
class field and `Step.Variant(name)` for an enum payload. Built-in collections
add `Index` and `Key` steps themselves. `allocated(result, source)` maps an
allocation failure through the decoder's error type.

## Errors and costs

`SerialError[E]` retains the original typed `cause`, a `path: Option[list[Step]]`, and
`complete: bool`. Paths run **innermost first**: reverse them for display from
the root. Appending a step can allocate. If allocation fails, or the path
already contains 1024 steps, `complete` becomes false and the original cause
and collected prefix remain intact. A root error needs no backing allocation.
`Nothing` represents a path with no collected steps; this avoids allocating
an empty list while constructing the error. Success does not allocate path entries.

`Fault` distinguishes unexpected kinds, missing/duplicate/unknown fields,
unknown variants, range failures, wrong tuple lengths, duplicate set elements,
and values a format cannot represent. `Decoder.reject(fault)` turns these
semantic failures into its concrete error type. `Decoder.allocation(cause)`
preserves format context for container allocation failures.

The protocols use scalar operations and explicit container boundaries.
`Decoder.null()` consumes a null and returns true, or leaves a non-null value
available and returns false. `next_element()` selects one array element, or
closes the array and returns false. `next_key()` selects an object's value,
or closes the object and returns `Nothing`. Consume the selected value before
advancing. `unknown_key()` consumes one complete selected value when ignoring
it. An encoder writes exactly one root value. After any failure, discard the
adapter; recovery and reuse are not supported. Format adapters own depth,
input, output, and tree budgets.

All dispatch is static. Containers allocate their decoded storage; adapters
may allocate parsing/output buffers and traversal stacks. The implementation
does not promise allocation-free strings or a performance improvement over a
specialized handwritten event loop. Compile-time, code-size and throughput
measurements remain a prerequisite for derivation.

## Acknowledgements

- [Swift Codable](https://www.swift.org/documentation/) informed the
  compiler-dispatched, format-neutral hook design recorded in the proposal;
  no Swift code or data ships in this module.
- [Kotlin serialization](https://kotlinlang.org/docs/serialization.html)
  informed the compiler-generated serializer design recorded in the proposal;
  no Kotlin code or data ships in this module.

# Types and data definitions

Primitive types are `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`,
`f64`, `bool`, `str`, and [`bytes`](bytes.md), the type of byte literals such
as `b"\x89PNG"`. There is no built-in `int`; use a width or a
[transparent alias](aliases.md). [Numbers](numbers.md) have local literal
inference and explicit conversions rather than implicit widening.

The unit type `()` denotes completion without data. It can be an expression,
function result, enum or tuple component, including `Option[()]` and
`Result[(), E]`, or a [channel message type](../../facilities/03-concurrency/channels.md#unit-messages).
Standalone unit bindings, parameters, collection elements, class fields and
generic data arguments are unsupported. A reference to unit, `&()` or `&mut ()`, is an
ordinary reference: borrowed unit payloads have this type, and a `&()` parameter
lends a unit argument like any other temporary. There is no `None` type or value.

[Tuples](tuples.md) are structural products. [Classes](classes.md) and
[enums](enums.md) are nominal: identical fields or payloads do not make separately
declared types interchangeable. Import aliases preserve a declaration's identity;
transparent aliases create none. Generic instances with different arguments have
different nominal identities.

[Dimensions](dimensions.md) are compile-time types with no values. Nominal
bases and normalized products, quotients, powers and exact roots can be arguments
to generic parameters constrained by `Dimension`.

Type inference stays within local expressions and calls; every function interface
declares its types. Collection, reference, callable, and generator types follow
their respective [facilities](../../facilities/index.md),
[ownership](../05-ownership/index.md), [function](../06-functions/index.md), and
[iteration](../09-iteration/index.md) contracts.

Data definitions may refer forward and recur through permitted
[storage boundaries](recursive-data.md). References cannot be stored in classes,
enums, or tuples. Generators can occur inside standard `Option` and `Result`
wrappers, but not class fields, user enum payloads, or tuple components.

[Quantities](quantities.md) combine numeric storage with a compile-time dimension
and check that arithmetic combines those dimensions correctly.

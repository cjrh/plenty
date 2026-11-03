# Heterogeneous collections

A heterogeneous collection holds values of different kinds. Plenty still gives
it one element type: an enum or an anonymous sum describes the alternatives.
The possible types are known at compile time.

Use the [classes and enums](../06-data/index.md) you have defined to choose a
representation:

| Collection | Representing different kinds of data |
| --- | --- |
| [Lists](lists.md) | Use an enum wrapper for a named interface, or `list[A | B]` for a choice of whole types. |
| [Sets](sets.md) | Use a named enum with supported key payloads, or keep separate typed sets. Anonymous sums are not supported as members. |
| [Dictionaries](dictionaries.md) | Keep one supported key type and use an enum or anonymous sum for mixed values. |

An **anonymous sum**, written `A | B`, holds one value of either type. It is not
a set collection: `set[T]` holds distinct values. An enum gives its alternatives
names and can define methods; an anonymous sum needs a `match` to use each
member's own interface.

These techniques do not create an open-ended collection accepting any type.
Adding a new kind means extending the enum or sum and handling it wherever
matching must be exhaustive. [Protocols](../09-generics/protocols.md) constrain
generic calls; they are not runtime element types such as `list[MyProtocol]`.

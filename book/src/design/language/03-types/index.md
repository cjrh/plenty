# Types and data definitions

Primitive types are `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`,
`f64`, `bool`, `str`, and [`bytes`](bytes.md), the type of byte literals such
as `b"\x89PNG"`. There is no built-in `int`; use a width or a
[transparent alias](aliases.md). [Numbers](numbers.md) have local literal
inference and explicit conversions rather than implicit widening.

The unit type `()` denotes completion without data. It has one value, also
written `()`, and is an ordinary value type: a binding, parameter, function
result, tuple component, class field, enum payload, collection element,
generator item, [channel message](../../facilities/03-concurrency/channels.md#unit-messages)
or type argument to any generic function, class, enum or protocol can be `()`.
So a generic `T` can be instantiated with `T = ()`, as in
`std.sync.SendBeforeError[()]`. Closures and generator expressions capture
unit, `fold` accepts a unit accumulator, and `match` on unit takes `_` or
`_ as name` cases. Unit values print as `()`, are equal to each
other, are copyable and own nothing. Unit is not a
[supported key type](../../facilities/01-builtins/collections.md): a set element or dictionary key
cannot be `()`. A reference to unit, `&()` or `&mut ()`, is an ordinary
reference. There is no `None` type or value.

```plenty
class Done[T]:
    value: T

def identity[T](value: T) -> T:
    value

def main() -> Result[(), AllocError | IoError]:
    marker = ()
    done = Done[()](identity(marker))
    signals: list[()] = [(), ()]?
    print(done.value, len(signals), marker == ())?
    Ok(())
```

```output
() 2 True
```

A stored unit value currently takes the same word as any scalar: a unit
parameter is passed in a register, and a unit field or list element takes a
value slot. Only a [unit channel](../../facilities/03-concurrency/channels.md#unit-messages)
stores no messages. Programs cannot observe the difference.

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

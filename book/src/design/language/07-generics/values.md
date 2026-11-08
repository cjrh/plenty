# Value generic parameters

Functions, classes, enums and methods can specialize on compile-time integers
and booleans as well as types. Write `const` before a value parameter:

```plenty
def selected[const Enabled: bool](value: i64) -> i64:
    if Enabled:
        return value
    0

def main() -> Result[(), IoError]:
    print(selected[True](7), selected[False](7))?
    Ok(())
```
```output
7 0
```

A value parameter's declared type is one of `i8`, `i16`, `i32`, `i64`, `u8`,
`u16`, `u32`, `u64` or `bool`. It may instead name a **preceding** type parameter
constrained by `IntType`, as in `[T: IntType, const lo: T, const hi: T]`.
This dependent form keeps a bound's width and signedness identical to `T`.
Aliases in the const parameter's own type annotation are not supported.
Protocols and their requirement parameters do not accept const parameters.

## Arguments and identity

Each value argument is an integer or boolean literal, an optionally negated
integer literal, a module constant (including an imported public constant), or
a bare value parameter being forwarded. Unsuffixed literals take the parameter's
declared type; suffixed literals and constants must match it exactly. There is
no widening. Floating-point values, strings, calls, numeric type constants such
as `i64.MAX`, and expressions such as `N + 1` are not value arguments. A module
constant may name a value computed by a supported constant initializer.

```plenty
const maximum: i64 = 4 * 4
class Tag[const N: i64]:
    def number(self) -> i64:
        N

def number[const N: i64](value: &Tag[N]) -> i64:
    value.number()

def main() -> Result[(), IoError]:
    tag: Tag[16] = Tag[maximum]()
    print(number(&tag))?
    Ok(())
```
```output
16
```

Instance identity compares resolved values, so `Tag[maximum]` and `Tag[16]`
are the same type. Different values remain different nominal types even when
no field uses the parameter. Aliases can name complete instances. Values add
no runtime fields and do not change copying, borrowing or cleanup rules.
A value argument cannot appear where a type argument is required.

## Bodies and inference

Within a specialized body, a const parameter is an immutable value of its
declared type. Reads become typed literals. Ordinary arithmetic on it uses the
usual [arithmetic contracts](../03-types/numbers.md#checked-blocks); const
parameters do not make a function a compile-time evaluator. A local binding
cannot reuse a generic parameter's name.

Structural inference learns values from nominal arguments: `Tag[N]` matched
against `Tag[16]` supplies `N = 16`. Repeated evidence must agree. An ordinary
integer argument's runtime value supplies no evidence, and an output-only
parameter needs an explicit argument unless an
[expected result](inference.md#expected-results) names it. As with type parameters, supply all
arguments explicitly or infer all of them. Methods supply only their own
parameters; the receiver already fixes its class or enum arguments.

Each distinct argument combination uses the existing specialization cache and
[instance limits](../../toolchain/02-architecture/emission.md). Value argument
expressions cannot grow recursively because bracket arithmetic is rejected.

[`Bounded[T, lo, hi]`](../03-types/bounded.md) is the first built-in consumer.
Inline arrays, derived sizes in signatures, and general per-instance static
assertions are not implemented; [B83](../../../backlog.md) tracks that scope.

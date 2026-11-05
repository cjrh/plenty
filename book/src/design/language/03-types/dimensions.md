# Compile-time dimensions

A dimension identifies a physical dimension without introducing a runtime value.
Declare a nominal base with its coherent-unit symbol, or a transparent derived
dimension with an expression:

```plenty
dimension Length("m")
dimension Time("s")
dimension Velocity = Length / Time
dimension Acceleration = Length / Pow[Time, 2]

@copy
class Sample[D: Dimension]:
    value: f64

def reading[D: Dimension](sample: Sample[D]) -> f64:
    sample.value

def main() -> Result[(), IoError]:
    speed: Sample[Velocity] = Sample[Length / Time](3.0)
    print(reading(speed))?
    Ok(())
```
```output
3.0
```

`Sample` demonstrates a dimension argument. Its field is an ordinary `f64`;
this class does not supply dimension-aware arithmetic or unit conversion.
For physical values and arithmetic, use the built-in
[`Quantity[T, D]`](quantities.md).

## Declarations and identity

`dimension` is a keyword. Declarations belong at module scope and follow ordinary
[module visibility](../02-source/visibility.md): `pub dimension Length("m")` is
exported. Bases in different modules differ even when their names and symbols
match. Import aliases preserve identity. A public derived dimension or signature
cannot expose a private base dimension.

A derived declaration creates no new identity. Forward references work; cycles
are rejected. `One` is the built-in dimensionless dimension. Coherent-unit
symbols are retained as metadata; they do not select a conversion or change
dimension equality.

Dimensions have no values. A dimension can be an argument to a generic parameter
constrained by `Dimension`, but cannot be a parameter's value type, a return
value, a field, a collection element, or an ordinary `type` alias target.
`CopyType` describes values and does not admit dimensions.

```plenty-error
dimension Length("m")
def bad(length: Length) -> ():
    pass
def main() -> ():
    pass
```
```error
dimension `Length` has no values
```

## Algebra

| Expression | Meaning |
| --- | --- |
| `One` | Empty product |
| Base, derived name, or dimension parameter | The named dimension |
| `A * B`, `A / B` | Product or quotient, left-associative; parentheses group |
| `Pow[A, n]` | Integer power |
| `Root[A, n]` | Exact integer root; `n` must be nonzero |

`n` is an unsuffixed integer literal within `i64` range, optionally negative.
Decimal, hexadecimal, octal, binary, and ordinary integer separators are
supported. Negative roots take the reciprocal of the corresponding positive
root. Every base exponent must divide exactly by the root degree.

`Pow` and `Root` are fallback type-operator names. A local declaration or import
with either name keeps its ordinary meaning; for example, importing `std.fs.Root`
as `FsRoot` leaves the dimension operator `Root` available.

The normal form is a sorted list of nominal bases with nonzero `i8` exponents.
Products commute and associate when their intermediate exponents stay in range;
division cancels exactly. Each subexpression must fit `-128..127`: a later
cancellation does not rescue an overflowing intermediate result. Normal forms
use base names and `Pow` terms; aliases do not change type identity.

```plenty-error
dimension Length("m")
dimension Invalid = Root[Pow[Length, 3], 2]
def main() -> ():
    pass
```
```error
not a whole dimension
```

## Diagnostic alias notes

Type mismatches and quantity dimension errors show the normalized dimensions.
When a matching derived alias is visible, a note connects that normal form to
its source spelling:

```plenty-error
dimension Length("m")
dimension Time("s")
dimension Velocity = Length / Time
def main() -> ():
    speed: Quantity[f64, Velocity] = Quantity[f64, Length](1.0)
```
```error
note: dimension alias `Velocity` = `Length * Pow[Time, -1]`
```

The first matching alias in declaration/import order is used. A namespace
import considers exported aliases in their declaration order. Notes use the
written import binding, such as `Fast` for `from units import Velocity as Fast`
or `u.Velocity` for `import units as u`. Private or unimported aliases from other
modules are excluded, as are names shadowed by generic parameters. Aliases of
`One` or of a single base work the same way.

The scope belongs to the expression or type that failed. A generic body error
can therefore explain an alias in the defining module while the diagnostic
also identifies the call that required that instance. Nested types and module
constants receive the same notes. Errors encountered while resolving dimension
declarations can explain aliases that have already resolved.

These notes do not change equality, specialization, coherent-unit output, or
normalized [quantity debug formatting](quantities.md#formatting).

## Generic dimensions

`D: Dimension` accepts dimensions in functions, classes, and enums. A bare
dimension parameter in an argument type can be inferred structurally, as in
`reading(sample)`. Explicit arguments use `reading[Velocity](sample)`.

In a function parameter type, a dimension argument must be concrete or a bare
parameter. An expression involving a parameter, such as `Sample[D / Time]`, is
rejected even for explicit calls. Return types and bodies may use dimension
expressions, evaluated after substitution for each instance. An invalid root
or overflowing exponent then reports the instantiation that required it.

Dimension arguments distinguish generic instances but add no fields or runtime
metadata to their values. Specializations differing only in dimensions are not
currently shared. See [generic specialization](../07-generics/index.md).

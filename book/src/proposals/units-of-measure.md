# Units of measure

**Status: Partly implemented.** This page proposes compile-time dimensional
analysis for Plenty: quantities whose physical dimension is part of the type,
checked by the compiler and erased before code generation. Milestone 1, the
language, shipped through issue #701 (local PRs #885, #887–#891); see
[dimensions](../design/language/03-types/dimensions.md),
[quantities](../design/language/03-types/quantities.md) and
[where the implementation differs](#implementation). Milestone 2's
[`std.units`](../design/stdlib/units.md) shipped without `wrap_list` and
`strip_list`; those and shared instance bodies remain under
[B97](../backlog.md#units-of-measure). Companion
pages record the [precedents](units-of-measure/precedents.md) and the
[spike evidence](units-of-measure/evidence.md). Researched against local
`main` at `bd407f05` on 2026-11-03; reconciled with `eb772202`.

## Goal

A program that adds a length to a time, or passes an acceleration where a
velocity is expected, should fail to compile. A program that passes these
checks should run exactly as fast as the same program written with `f64`, and
allocate nothing more.

This example needs `std.units` (milestone 2). `Quantity` is built in and
needs no import.

```text
import std.units as u
from std.units import Length, Time, Velocity

def speed(d: Quantity[f64, Length], t: Quantity[f64, Time]) -> Quantity[f64, Velocity]:
    d / t

def main() -> Result[(), IoError]:
    d = 3.2 * u.km
    t = 41.0 * u.minute
    v = speed(d, t)
    print(v, v.to(u.km / u.hour))?      # 1.3008130081300813 m/s 4.682926829268292
    print(d + t)?                       # compile-time error: Length + Time
    Ok(())
```

## Recommendation

1. **Dimensions are a compiler-known kind of type.** A module declares base
   dimensions (`dimension Length("m")`) and names derived ones
   (`dimension Velocity = Length / Time`). The compiler normalizes every
   dimension expression to a product of base dimensions with integer
   exponents, so `Length / Time * Time` is `Length`.
2. **`Quantity[T, D]` is a built-in numeric wrapper,** like `NonZero[T]`. It
   has `T`'s representation. The compiler defines its operators: `+`, `-` and
   comparisons need equal dimensions; `*` and `/` combine them.
3. **The type carries the dimension, not the unit.** A value is stored in the
   coherent SI unit of its dimension (metres, seconds, kilograms). `km`,
   `minute` and `pound` are constants, and `q.to(unit)` reads a value back in
   any unit. This is the `uom` and GNAT model.
4. **Storage types compose.** `T` may be any number type, including
   `Positive[i64]`, `NonNegative[i64]` and `NonZero[i64]`. An operation's
   storage result is whatever `T`'s own operator returns; its dimension comes
   from the dimension algebra. Units never drop out of a result.
5. **`std.units` declares SI** in Plenty: the seven base dimensions, named
   derived dimensions, prefixed and common units, CODATA constants, and
   dimension-aware `sqrt`, `hypot` and friends.

The recommendation needs no value generics, no type-level integers and no
generic operator hooks, which Plenty lacks; see
[what the language could do before](#what-the-language-could-do-before).

## What the language could do before

The [spikes](units-of-measure/evidence.md) ran on the release compiler built
from `main` on 2026-11-02, before issue #701. Their library workarounds still
behave as shown; the built-in `Quantity[T, D]` now covers the rows marked
**No** and **Partly**.

| Need | Today | Evidence |
| --- | --- | --- |
| Distinct types per dimension | Phantom type parameters: `Q[f64, Length]` and `Q[f64, Time]` are different types | Spike 1 |
| Same-dimension `+`, `-`, `<` | `@newtype` forwarding over a phantom-tagged class; mixing dimensions is rejected | Spikes 1, 2 |
| Code generic over one dimension | `def total[T, D](xs: &list[Q[T, D]]) -> Q[T, D]` | Spike 7 |
| Zero cost | One-field classes are represented as their field; operator hooks on a one-`f64` class compile to the same instructions as `a.x + b.x` ([operator lowering](../design/language/04-expressions/operators-implementation.md#bounded-optimization-and-evidence)) | Reference |
| `Length * Length -> Area`, generically | **No.** Operator hooks cannot have type parameters, a class has one `__mul__` signature, and there are no associated types or type-level arithmetic | Spike 4 |
| A domain inside a quantity | **Partly.** `@newtype` forwards `Positive + Positive` as `Positive`, but `Positive - Positive` returns a bare `i64`: the unit is lost | Spike 3 |
| `Positive[f64]` | **No.** Integer domains take only signed integers | Spike 5 |

So a phantom-tag library can check additive code today, but multiplication
and division, the core of dimensional analysis, need a language change.
Hand-written classes per dimension pair (spike 6) work for a handful of
quantities and do not scale: each class can declare only one `__mul__`.

## Options

| Option | Benefit | Cost |
| --- | --- | --- |
| A. Phantom-tag library with `@newtype` (works today) | No language change | Additive checks only; products need a hand-written function per pair |
| B. `std` library over [value generics](value-generics.md): `Quantity[T, const L: i8, const M: i8, const T: i8, ...]` | Reuses B83 | Needs B83, its deferred derived-size expressions (`L + L2`), generic operator hooks with value parameters, and a second `__mul__` for scalars. Seven fixed base dimensions, no user dimensions. Types print as `Quantity[f64, 1, 0, -1, 0, 0, 0, 0]`; a generic function over one dimension takes seven parameters |
| C. Compiler-known dimensions and built-in `Quantity` (recommended) | Readable types and diagnostics; user dimensions; one parameter per generic dimension; no new operator-hook machinery | A new kind of type and dimension expressions in the checker |
| D. C plus F#-style inference through dimension expressions in parameter types | `def sqrt[D](x: Quantity[T, D * D]) -> Quantity[T, D]` infers `D` | Unification over abelian groups in the checker; later, if code needs it |
| E. Runtime units (Pint, astropy, `Measurement` in Swift) | No type-system work | Run-time checks and storage per value; errors appear only on executed paths |
| F. Units in the type (mp-units, Unitful): `km` and `m` are distinct types | Exact integer storage in any unit | Conversion and promotion rules between units of one dimension; many more types |

Rust is a useful warning about option B. `uom` encodes exponents with
`typenum` type-level integers because stable const generics cannot compute
`L1 + L2` in a signature; that needs the unstable `generic_const_exprs`. The
const-generic ports (`tiny-uom`, `const_unit_poc`) require nightly features
and stopped in 2020. Option B in Plenty would hit the same wall at the
[derived sizes](value-generics.md#decisions) that B83 deliberately defers.

C++ took the library route too: Boost.Units with template metaprogramming,
then mp-units with C++20 class-type template parameters, now proposed for
C++29 as P3045. Both are large libraries and rely on language features
Plenty does not have and should not add for this. Ada GNAT and F# build
dimensions into the compiler, as option C does. See the
[precedents](units-of-measure/precedents.md).

## Design

### Declaring dimensions

```text
dimension Length("m")
dimension Mass("kg")
dimension Time("s")
dimension Velocity = Length / Time
dimension Force = Mass * Length / Pow[Time, 2]
```

- A **base dimension** has a name and the symbol of its coherent unit. Its
  identity is nominal, like a class: `pkg.Length` and `std.units.Length`
  differ.
- A **derived dimension** is a transparent alias for a dimension expression,
  like a [type alias](../design/language/03-types/aliases.md). `Velocity` and
  `Length / Time` are the same type.
- `One` is the built-in dimensionless dimension, the empty product.
- `dimension` declarations are module-level and follow ordinary visibility
  (`pub dimension Length("m")`). `dimension` becomes a keyword; it is a
  declaration form, so this is the one source change existing programs could
  meet.

A dimension is a type with no values. It may appear only as a dimension
argument, so `Quantity[f64, Length]` is valid and `x: Length` is not.

### Dimension expressions

Wherever a dimension is expected, the compiler accepts:

| Form | Meaning |
| --- | --- |
| `Length`, `Velocity`, `D` | A base dimension, alias or dimension parameter |
| `A * B`, `A / B` | Product and quotient, left-associative, with parentheses |
| `Pow[A, n]` | Integer power; `n` is an integer literal, possibly negative |
| `Root[A, n]` | `n`th root; every exponent of `A` must be divisible by `n`; `n` is nonzero, and a negative `n` takes the reciprocal |
| `One` | Dimensionless |

The normal form is a list of (base dimension, nonzero `i8` exponent) pairs
sorted by base identity. Two dimensions are equal exactly when their normal
forms are equal. An exponent outside `i8` is a compile-time error, including
in an intermediate result that a later factor would cancel.

Exponents are integers. `Root` rejects a dimension it cannot divide exactly:
the square root of `Pow[Length, 3]` is an error, not `Length^(3/2)`. This
excludes units such as V/√Hz from noise spectral density. F# added
rational exponents in its fourth version for these cases; the normal form
could store rationals later without changing any spelling.

### Generic dimensions

`D: Dimension` declares a dimension parameter. `Dimension` is a built-in
constraint, like `IntType`; it admits exactly the dimension types.

```text
def mean[T: FloatType, D: Dimension](xs: &list[Quantity[T, D]]) -> Quantity[T, D]
def kinetic[M: Dimension, V: Dimension](m: Quantity[f64, M], v: Quantity[f64, V]) -> Quantity[f64, M * V * V]
def sqrt[T: FloatType, D: Dimension](x: Quantity[T, D]) -> Result[Quantity[T, Root[D, 2]], MathError]
```

One rule keeps inference structural, as it is today: **in parameter types, a
dimension argument is either concrete or a bare parameter.** Return types,
annotations and bodies may use any expression. Since Plenty checks bodies per
instance, `M * V * V` and `Root[D, 2]` are evaluated after substitution, where
every operand is known. This is the form B83 calls "expressions in
non-inferred output positions", applied to dimensions instead of integers.

A `Root` that does not divide is reported as an instance error at the call
that required it, through the existing
[instance diagnostics](../design/language/07-generics/index.md#diagnostics-from-instances):

```text
main.plenty:8:9: cannot instantiate `units.sqrt[f64, Pow[Length, 3]]`: `Root[Pow[Length, 3], 2]` is not a whole dimension
  note: `Length` has exponent 3, which 2 does not divide
```

Option D, inferring `D` from `x: Quantity[T, D * D]`, stays out until code
needs it. Option C programs remain valid under it.

### Quantities

`Quantity[T, D]` holds one `T` measured in the coherent unit of `D`. `T` is a
number type: an integer, a float, or an integer domain type. `D` is a
dimension.

| Operation | Result |
| --- | --- |
| `Quantity[f64, Length](3.0)` | 3 m. The constructor takes the coherent-unit value; prefer `3.0 * u.metre` in application code. |
| `q.get()` | The stored `T`, in coherent units |
| `q.to(unit)` | `(q / unit).get()`, for float storage: `unit` is a quantity of the same dimension, such as `u.km`. Mismatched dimensions are a compile-time error. |
| `a + b`, `a - b` | Both `Quantity[T, D]`; result `Quantity[R, D]`, where `R` is the type `T + T` or `T - T` returns |
| `a * b`, `a / b` | `Quantity[R, A * B]` and `Quantity[R, A / B]`; `/` needs float storage |
| `k * q`, `q * k`, `q / k` | A scalar `T` scales without changing the dimension |
| `k / q` | `Quantity[R, One / D]` |
| `a // b`, `a % b` | Integer storage: `//` divides dimensions, `%` keeps `a`'s and needs equal dimensions |
| `-q`, `+q` | As for `T`, dimension unchanged |
| `<`, `<=`, `==`, ... | Equal dimensions; `bool` |
| `+=`, `-=` | Equal dimensions. `*=` and `/=` take a scalar `T` only, since the dimension of a variable cannot change. |
| `print(q)`, `f"{q}"` | The value, then the coherent unit: `1.3 m/s`, `9.81 m/s^2`. A format spec applies to the number: `f"{q:.2f}"` is `1.30 m/s`. |
| `debug` | `Quantity[f64, Length * Pow[Time, -1]](1.3)`, the normal form |

Rules with no exceptions:

- **Scalars only scale.** A plain `T`, including a literal, may multiply or
  divide a quantity, never be added to it or compared with it. `q > 0.0` is
  an error; write `q > 0.0 * u.metre`. F#'s `0.0<_>` wildcard is the
  exception this rule avoids.
- **Dimensionless is still a quantity.** `d / d` is `Quantity[f64, One]`.
  `.get()` returns the `f64`. There is no implicit conversion.
- **Storage types never mix.** `Quantity[f32, D] + Quantity[f64, D]` is an
  error, as `f32 + f64` is.

Overflow, division by zero and `checked:` behave as they do for `T`. A
quantity operation is `T`'s operation, so a `checked:` block reports
`OverflowError[T]` or `ZeroDivisionError`.

### Unit constants and conversions

A module constant may have a quantity type, with an initializer built from
literals and other quantity constants:

```text
pub const metre: Quantity[f64, Length] = Quantity[f64, Length](1.0)
pub const km: Quantity[f64, Length] = 1000.0 * metre
pub const hour: Quantity[f64, Time] = 3600.0 * second
pub const newton: Quantity[f64, Force] = kg * metre / (second * second)
```

Construction multiplies by the unit, and `to` divides by it. Both are one
floating-point operation, folded at compile time when the operands are
constants. Converting through the coherent unit can round: `0.1 * u.foot`
then `.to(u.foot)` need not return exactly `0.1`. Programs that need exact
values in a non-coherent unit should declare their own dimension or keep that
unit's integer count, as `std.time.Duration` keeps nanoseconds.

Unit constants are `f64`. `f32` quantities convert with
`Quantity[f32, D].from(q)`, which narrows the stored value as `f32(x)` does;
`Quantity[f64, D].from(q)` widens.

### Domain storage

The user-facing question was whether `Positive`, `NonNegative` and `NonZero`
can carry units. They can, by nesting the domain inside the quantity and
taking the storage result from the domain's own rules:

| Expression, with `p: Quantity[Positive[i64], Length]` | Result |
| --- | --- |
| `p + p`, `p * p` | `Quantity[Positive[i64], Length]`, `Quantity[Positive[i64], Pow[Length, 2]]` |
| `p - p`, `-p` | `Quantity[i64, Length]`: the domain drops, the unit stays |
| `n // p`, with `n: Quantity[i64, Area]` | `Quantity[i64, Length]`, without the zero or `MIN // -1` check |
| `Quantity[Positive[i64], Length](0)` | Compile-time error, as for `Positive[i64]` |
| `Quantity[Positive[i64], Length].try_from(k)` | `Result[..., RangeError]` for a computed `k` |
| `Positive[Quantity[i64, Length]]` | Rejected: one nesting order, the domain inside |

Today's `@newtype` forwarding does the opposite for `-`: it returns the bare
`i64` (spike 3). The built-in rule rewraps every result.

Integer storage counts whole coherent units, so `Quantity[i64, Length]` holds
whole metres. That suits counts such as pixels, bytes or items, declared as
user dimensions, more than SI quantities.

Float domains do not exist. Mass, absolute temperature and the argument of a
logarithm would benefit from a `Positive[f64]`. Its rules differ from the
integer domain: it must exclude NaN, and `*` cannot preserve it because a
product of two small positive values can underflow to zero. That belongs in a
separate proposal. The storage rule above means a later float domain works
inside quantities with no further units design.

### `std.units`

Written in Plenty, as declarations and small functions:

| Part | Contents |
| --- | --- |
| Base dimensions | `Length` (m), `Mass` (kg), `Time` (s), `Current` (A), `Temperature` (K), `Amount` (mol), `Luminosity` (cd) |
| Derived dimensions | `Area`, `Volume`, `Velocity`, `Acceleration`, `Force`, `Pressure`, `Energy`, `Power`, `Charge`, `Voltage`, `Resistance`, `Frequency`, `Density`, and the other named SI derived quantities |
| Units | SI prefixes on the base and named derived units (`km`, `ms`, `kN`, `MPa`, `kWh`), plus `minute`, `hour`, `day`, `litre`, `tonne`, `bar`, `atm`, `eV`, and the international foot, inch, mile, pound and pound-force |
| Constants | The exact SI defining constants (`c`, `h`, `e`, `k_B`, `N_A`) and CODATA values such as `G`, typed |
| Functions | `sqrt`, `cbrt`, `abs`, `hypot`, `min`, `max`, `clamp`, `mean`, `sum`; `exp`, `log` and trigonometry take `Quantity[f64, One]` |
| Temperature | `from_celsius`, `to_celsius`, `from_fahrenheit`, `to_fahrenheit` (affine; see [out of scope](#out-of-scope)) |
| Time | `from_duration(&Duration) -> Quantity[f64, Time]` and a fallible `to_duration` |
| Lists | `wrap_list[D](xs: list[T]) -> list[Quantity[T, D]]` and `strip_list`, consuming the list without copying, since the element layouts are identical |

Angles are dimensionless, as in SI: `u.degree` is `Quantity[f64, One]`
holding π/180. A program that wants angles checked declares
`dimension Angle("rad")` and converts at trigonometric calls.

### Working with the numeric libraries

| Library | Boundary |
| --- | --- |
| `std.dataframe` | `units.wrap_list[Length](frame.values_f64("x")?)` gives typed values with no copy beyond `values_f64` |
| `std.plot` | `Column.Num(units.strip_list(qs))`. An axis label can come from the unit: `f"distance ({u.km.symbol()})"`. Automatic labels are later work. |
| `std.numeric` ([proposal](numerics.md)) | Solvers keep `f64` callbacks. A callback strips and rewraps at its boundary. Quantity-generic solvers wait for evidence. |
| `std.linalg` | `Matrix` stays `f64`. Elements of one matrix often have different dimensions, which a single `D` cannot express; out of scope |

### Cost

- **Run time: none.** `Quantity[T, D]` has `T`'s representation, operators
  lower to `T`'s instructions, and unit constants fold. Only `print` and
  formatting read the dimension, as a static symbol string.
- **Code size.** Generic instances that differ only in dimension arguments
  have identical bodies unless they format a quantity. The implementation
  should emit one body for them. Rust and C++ rely on the linker's identical
  code folding for this; Plenty's per-instance checking makes it an explicit
  step after checking.
- **Compile time.** A normal form is a short sorted list, interned with other
  types. Each dimension expression is normalized once per instance.

## Implementation

Milestone 1 shipped as designed, with these differences. The reference pages
are authoritative.

| Topic | This design | Implemented |
| --- | --- | --- |
| Integer division | `a // b` where `T` supports it | `/` is float-only and `//` integer-only for quantities; integer storage has no `to`, so use `//` and `.get()` |
| `Root` degree | Positive `n` | Nonzero `n`; negative takes the reciprocal |
| `Pow`, `Root` | Reserved forms | Fallback names that a local declaration or import shadows |
| Exponents | `i8` normal form | Literal `n` within `i64`; every intermediate exponent must fit `i8` |
| Debug output | `Quantity[f64, Length / Time](1.3)` | Normal form: `Quantity[f64, Length * Pow[Time, -1]](1.3)`; alias notes appear only in diagnostics |
| `checked:` | `OverflowError[T]` | Also `ZeroDivisionError`, as for the underlying arithmetic |
| Constants | Literals and quantity constants | Also casts of extracted values, `.get()` and float `.to`; no user function calls |
| Zero-cost evidence | Release instructions match `f64` | Matched for `f32` and `f64` `+`, `*` and `.to` on x86-64; not measured on other targets |
| Instance sharing | Milestone 1 if cheap | Not done; instances that differ only in dimension have separate bodies |
| `std.units` lists | `wrap_list` and `strip_list` | Not done: retagging a list's element type without copying needs compiler support |
| `std.units` storage | Unstated | Units, constants and the `std.math`-based functions use `f64`; `abs` and `clamp` accept any storage |
| `min`, `max` | In `std.units` | Not repeated: the generic `std.math.min` and `max` already accept quantities |
| `mean` | Unstated | Returns `Option`, `Nothing` for an empty list, as `std.math.min_all` does |
| Symbol order | SI order, `kg*m/s^2` | Sorted by qualified base name: a newton prints `m*kg/s^2` |

## Out of scope

| Feature | Reason |
| --- | --- |
| Units in the type (option F) | Conversion and promotion rules for one dimension; the dimension-only model covers the checks |
| Affine quantities (°C, °F, positions, points in time) | `from_celsius` converts at the boundary. mp-units models these as `quantity_point`; add one only with use cases. `std.time` already separates `Instant` from `Duration`. |
| Quantity kinds: torque vs energy, Hz vs Bq, `rad` vs `sr` | SI gives them one dimension. A user dimension separates them where it matters. |
| Rational exponents | Integer exponents first; see [dimension expressions](#dimension-expressions) |
| Logarithmic units (dB, neper) | Not multiplicative; ordinary functions |
| Matrices and dataframes with per-column units | Needs a dimension per column or element, as UnitfulTensors.jl does; later |
| Dimension expressions in parameter types (option D) | Inference by abelian unification; later if needed |

## Milestones

1. **Language. Delivered** (issue #701). `dimension` declarations, the `Dimension` constraint,
   dimension expressions with `Pow` and `Root`, normal forms and identity, the
   parameter-type rule, built-in `Quantity[T, D]` for every number and integer
   domain storage type, operators, `get`, `to`, `try_from`, printing,
   quantity module constants and diagnostics. Reference page, tutorial lesson
   and cheat-sheet entry.
2. **`std.units`. Delivered** except `wrap_list` and `strip_list`. SI
   dimensions, units, constants and functions; temperature and `Duration`
   conversions.
3. **Measured follow-ups.** Shared bodies for instances differing only in
   dimension, if milestone 1 did not include them; plot axis labels from
   units; then any later item with a use case.

## Testing and acceptance

- **Algebra:** normalization is commutative and associative, cancels exactly,
  and `Root` rejects every non-divisible exponent. Property tests over random
  expressions compare normal forms against a reference evaluator.
- **Operators:** every operator for every storage type, including each domain
  and `checked:` blocks, with expected storage and dimension results.
- **Rejections:** each rule above has a diagnostic test, including additive
  scalars, mixed storage, `Positive[Quantity[...]]`, dimensions in value
  positions and expressions in parameter types.
- **Zero cost:** the operator-lowering probe extended to quantities:
  `a + b`, `a * b` and `q.to(u.km)` on `Quantity[f64, D]` must emit the same
  release instructions as the `f64` program. Allocation-denial tests cover
  quantity arithmetic and `wrap_list`.
- **Library:** `std.units` constants checked against the SI Brochure (9th
  edition) and CODATA 2022; conversions round-trip within one ulp where the
  factor is exact.

## Risks and open questions

- **Diagnostics through aliases.** Resolved: diagnostics show the normal form
  with a note naming the first visible matching alias in declaration or import
  order, using the importing module's spelling.
- **Symbol spelling.** Resolved for now: ASCII base symbols in normal-form
  order, which sorts by qualified base name. A newton prints `m*kg/s^2`, not
  the conventional `kg*m/s^2`. SI order and named-unit display (`J`) are later
  work.
- **The `dimension` keyword** may conflict with existing identifiers outside
  the repository. `Pow` and `Root` are not keywords: a local declaration or
  import with either name keeps its ordinary meaning.
- **Learner cost.** Units add a concept. The tutorial lesson should come after
  generics and teach the rules above without the algebra's internals.

# Units of measure: precedents

Research for the [units of measure proposal](../units-of-measure.md),
gathered on 2026-11-03. Each entry records the model, what it costs at run
time, and what it means for Plenty. It contains no Plenty examples.

## Summary

| System | Where dimensions live | Run-time cost | Unit in type? | Generic over dimension |
| --- | --- | --- | --- | --- |
| Rust `uom` | Library; `typenum` type-level integers | None; values normalized to base units | No | Yes, with trait bounds on `typenum` arithmetic |
| Rust const-generic ports | Library; `const` exponents | None | No | Needs nightly `generic_const_exprs` |
| C++ Boost.Units | Library; template metaprogramming | None | Unit system in type | Yes, verbose |
| C++ mp-units (P3045) | Library; C++20 class-type template parameters | None | Yes | Yes, with concepts |
| F# units of measure | Compiler; abelian-group unification | None; erased | Units, not dimensions | Yes, inferred |
| Ada (GNAT) | Compiler aspects on subtypes | None | No | Limited |
| Julia Unitful.jl | Library; type parameters, compiler specializes | None for homogeneous arrays | Yes | Yes, through dispatch |
| Python Pint, astropy | Run time | Per operation and per value | — | — |

## Rust

**`uom` 0.38.** Quantities are generic over a dimension type whose seven
exponents are `typenum` integers, the storage type and a unit system.
Values are normalized to the system's base units at construction, so
`Length::new::<kilometer>(3.2)` stores 3200 m; arithmetic is then plain
storage arithmetic. Results of `*` and `/` come from trait impls that add or
subtract exponents with `typenum` operators. Errors name `typenum` types such
as `PInt<UInt<UTerm, B1>>`. Plenty takes the normalization and the
dimension-only type; it avoids the type-level integer encoding.
Source: <https://docs.rs/crate/uom/latest>.

**`tiny-uom` 0.1 (2020)** and similar experiments replace `typenum` with
const generics. Computing `L1 + L2` in a signature needs the unstable
`generic_const_exprs` (then `const_evaluatable_checked`), so they require
nightly Rust and have not reached stable use. This is the problem B83 avoids by
deferring derived sizes.
Source: <https://docs.rs/crate/tiny-uom/latest/source/README.md>.

## C++

**Barton and Nackman** (1994) showed dimensional analysis through template
parameters. **Boost.Units** generalized it with template metaprogramming:
zero cost, long compile times and diagnostics.

**mp-units** uses C++20 class types as template parameters, so a quantity is
`quantity<si::metre / si::second, double>` with readable type names. It
distinguishes quantity kinds (`isq::height` vs `isq::width`), separates
points from differences (`quantity_point`, needed for °C), and keeps units in
the type, converting on assignment where that is exact. P3045 proposes it for
the C++ standard library; R7 is dated February 2026. The maintainer's reports
state that LEWG gave unanimous support at the Croydon meeting (March 2026)
for the core, quantity kinds, the affine space and text output, and that the
Brno meeting (June 2026) saw a new paper on non-negative absolute
quantities. These are the maintainer's reports, not committee minutes.
Sources:
<https://www.open-std.org/jtc1/sc22/wg21/docs/papers/2026/p3045r7>,
<https://github.com/mpusz/mp-units>,
<https://mpusz.github.io/mp-units/HEAD/blog/2026/03/28/report-from-the-croydon-2026-iso-c-committee-meeting>,
<https://mpusz.github.io/mp-units/HEAD/blog/2026/06/13/report-from-the-brno-2026-iso-c-committee-meeting/>.

The non-negative paper is the closest precedent for this proposal's
[domain storage](../units-of-measure.md#domain-storage): mp-units is adding
range constraints to quantities rather than wrapping quantities in a
separate range type.

## F#

Andrew Kennedy's units of measure are part of the F# compiler. `float<m/s>`
is a float with a unit; units form an abelian group, and inference solves
equations over it, so `let sqr x = x * x` gets the type
`float<'u> -> float<'u ^ 2>`. Units are erased after checking: a
`float<m>` is a `float` at run time. F# 4.0 added rational exponents
(`cm Hz^(1/2) / W`). F# writes literal zero as `0.0<_>` to unify with any
unit; this proposal rejects that exception and requires a typed zero.

F# puts units, not dimensions, in the type: `float<km>` and `float<m>` are
different, converted by explicit multiplication. Option F in the proposal
records why Plenty does not.
Sources:
<https://learn.microsoft.com/en-us/archive/blogs/andrewkennedy/units-of-measure-in-f-part-three-generic-units>,
<https://fsharp.github.io/fslang-spec/units-of-measure>,
<https://www.infoq.com/news/2015/04/FSharp-4/>.

## Ada (GNAT)

GNAT adds `Dimension_System` and `Dimension` aspects. A type declares the
base dimensions; subtypes declare exponents; `System.Dim.Mks` defines SI.
The compiler checks dimensions statically and generates no extra code.
It is the closest precedent for option C: a small compiler extension rather
than a type-level library, with dimension-only types.
Sources:
<https://gcc.gnu.org/onlinedocs/gnat_ugn/Performing-Dimensionality-Analysis-in-GNAT.html>,
<https://www.adacore.com/uploads/blog/dc.pdf>.

## Julia

**Unitful.jl** encodes units and dimensions as type parameters, and Julia's
specializing compiler removes them for concrete types. Arrays of one
quantity type are as fast as plain arrays. Users report long type names,
slow arrays mixing units, and the need for UnitfulTensors.jl for matrices
with per-element units. Plenty's proposal leaves matrices with units out of
scope for the same reason.
Sources:
<https://discourse.julialang.org/t/physical-units-in-julia/76825>,
<https://discourse.julialang.org/t/performance-of-unitful-arrays/52635>,
<https://discourse.julialang.org/t/ann-unitfultensors-jl-efficient-arrays-with-units/114028>.

## Python

**Pint** and **astropy.units** attach a unit object to each value and check
at run time. They are flexible and cost storage and time per operation;
errors appear only on executed paths. They are option E in the proposal.

## Motivating failure

The Mars Climate Orbiter was lost in 1999 after ground software produced
impulse in pound-force seconds where the navigation software expected newton
seconds. Both are impulse. A dimension-only type system would not have caught
this, since the values had the same dimension; normalizing every value to a
coherent unit at construction would have. This is why the proposal stores
coherent-unit values and makes `3.0 * u.lbf * u.second` the only way to write a
pound-force second.

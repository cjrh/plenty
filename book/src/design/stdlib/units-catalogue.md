# std.units.catalogue

`import std.units.catalogue as uc` adds every SI prefix on every SI unit and
the other named units of [NIST SP 811](https://www.nist.gov/pml/special-publication-811)
(2008), appendix B, to [`std.units`](units.md). Its units are the same
`Quantity[f64, D]` values over the `std.units` dimensions, so they mix freely
with `std.units` units and constants. Nothing in it allocates.

```plenty
import std.units as u
import std.units.catalogue as uc

def main() -> Result[(), IoError | AllocError]:
    gold = 3.0 * uc.troy_ounce
    print(f"{gold.to(u.g):.2f} g"?)?
    battery = 3000.0 * uc.milliampere_hour * (3.7 * u.V)
    print(f"{battery.to(u.Wh):.1f} Wh"?)?
    lot = 2.5 * uc.survey_acre
    print(f"{lot.to(u.acre):.6f} international acres"?)?
    print(uc.Gm.to(u.km), uc.fs.to(u.ps))?
    Ok(())
```
```output
93.31 g
11.1 Wh
2.500010 international acres
1000000.0 0.001
```

The module holds 768 constants. Constant evaluation costs about 10 ms of
compile time for a program that imports it, and nothing for one that does not.

## Prefixed SI units

The module defines each of the 24 SI prefixes, quecto (`q`, 10⁻³⁰) to quetta
(`Q`, 10³⁰), on each SI base unit and each coherent derived unit with a special
name except the steradian and the degree Celsius: `g` (prefixes apply to the
gram), `m`, `s`, `A`, `K`, `mol`, `cd`, `rad`, `Hz`, `N`, `Pa`, `J`, `W`, `C`,
`V`, `F`, `ohm`, `S`, `Wb`, `T`, `H`, `lm`, `lx`, `Bq`, `Gy`, `Sv` and `kat`.
Names follow `std.units`: prefix symbol then unit symbol, with micro written
`u` and deca `da`, as in `ug`, `dam`, `GPa` and `Mohm`. Each value is a
power-of-ten literal times the coherent unit, so it is the nearest `f64` to the
exact multiple.

Two prefixed symbols cannot be names. The attosecond is `attosecond`, because
`as` is a keyword, and the milliohm is `milliohm`, because `std.units` rejects
`mohm` as readable as milli or mega.

Prefixed forms that `std.units` also defines, such as `km`, hold the same
values. The catalogue also adds `microlitre`, `centilitre`, `decilitre`,
`hectolitre`, `kilolitre`, `megalitre`, `meV`, `TeV` and `TWh`.

## Named units

| Group | Units |
|---|---|
| Length | `fermi`, `micron`, `mil`, `microinch`, `point`, `pica`, `printers_point`, `printers_pica`, `survey_foot`, `survey_mile`, `chain`, `rod`, `fathom` |
| Area | `are`, `barn`, `circular_mil`, `survey_acre`, `darcy` |
| Volume | `stere`, `acre_foot`, `barrel`, `quart`, `pint`, `cup`, `gill`, `fluid_ounce`, `tablespoon`, `teaspoon`, `bushel`, `peck`, `dry_quart`, `dry_pint`, `imperial_gallon`, `imperial_gill`, `imperial_fluid_ounce`, `cord`, `register_ton` |
| Mass | `carat`, `grain`, `pennyweight`, `troy_ounce`, `troy_pound`, `short_hundredweight`, `long_hundredweight`, `short_ton`, `long_ton`, `assay_ton`, `slug` |
| Time | `shake`, `sidereal_day`, `sidereal_hour`, `sidereal_minute`, `sidereal_second`, `sidereal_year`, `tropical_year` |
| Force | `dyne`, `kilogram_force`, `ounce_force`, `poundal`, `kip`, `ton_force` |
| Pressure | `technical_atmosphere`, `ksi`, `cmHg`, `inHg`, `ftHg`, `mmH2O`, `cmH2O`, `inH2O`, `ftH2O` |
| Energy | `erg`, `calorie_IT`, `kilocalorie_IT`, `btu_th`, `therm_EC`, `therm_US`, `quad`, `ton_of_tnt` |
| Power | `horsepower_electric`, `horsepower_metric`, `ton_of_refrigeration` |
| Viscosity | `poise`, `centipoise`, `rhe`, `stokes`, `centistokes` |
| Electricity and magnetism | `ampere_hour`, `milliampere_hour`, `faraday`, `mho`, `debye`, `gauss`, `gamma`, `maxwell`, `oersted` |
| Radiation | `curie`, `rad_dose`, `rem`, `roentgen` |
| Light | `footcandle`, `phot`, `stilb`, `lambert`, `footlambert` |
| Angle | `revolution`, `gon`, `angular_mil` |
| Other | `gal`, `kayser`, `tex`, `denier` |

Each unit is built from its definition rather than from SP 811's rounded
factor: `survey_foot` is 1200/3937 m, `survey_acre` 43,560 square survey feet,
`btu_th` the thermochemical calorie per gram kelvin times a pound and a degree
Fahrenheit, and `slug` one pound-force second squared per foot. A unit whose
size is measured rather than defined, such as `sidereal_day` (86,164.0905 s)
or `tropical_year` (365.24219 days, J2000), uses the current value.

Some units need a choice of name or variant:

- `point` and `pica` are the desktop-publishing point (1/72 in) and pica
  (1/6 in); `printers_point` (0.013837 in) and `printers_pica` are the older
  American printer's units.
- `rad_dose` is the rad of absorbed dose (0.01 Gy). `u.rad` is the radian.
- `gal` is the gal of acceleration (0.01 m/s²); the US gallon is `u.gallon`.
- `quart`, `pint`, `cup`, `gill`, `fluid_ounce`, `tablespoon` and `teaspoon`
  are US liquid measures; `dry_quart`, `dry_pint`, `peck` and `bushel` are US
  dry measures. Imperial measures have an `imperial_` prefix.
- `calorie_IT` and `kilocalorie_IT` are International Table calories;
  `u.calorie` is the thermochemical calorie. `btu_th` is the thermochemical
  BTU; `u.btu` is the International Table BTU.
- The conventional mercury and water columns (`cmHg`, `inH2O` and so on) use
  13,595.1 kg/m³ and 1,000 kg/m³ under standard gravity.
- `faraday` is e·N_A·mol, exact in the 2019 SI; SP 811 printed the earlier
  CODATA value.

Compound units have no constants: write `u.btu * u.inch / (u.s * u.foot *
u.foot * u.rankine)` for a BTU-inch per second square foot degree Fahrenheit.
Square and cubic customary units are products too, such as `u.foot * u.foot`.

## Dimensions

The catalogue declares the dimensions its units need that `std.units` lacks.
Each names a distinct normal form, as the `std.units` aliases do.

| Dimension | Definition | Units |
|---|---|---|
| `LinearDensity` | `u.Mass / u.Length` | `tex`, `denier` |
| `ElectricDipoleMoment` | `u.Charge * u.Length` | `debye` |
| `MagneticFieldStrength` | `u.Current / u.Length` | `oersted` |
| `Exposure` | `u.Charge / u.Mass` | `roentgen` |
| `Luminance` | `u.Luminosity / u.Area` | `stilb`, `lambert`, `footlambert` |

`rhe` has type `Quantity[f64, One / u.DynamicViscosity]`.

## Omissions

- The CGS electrostatic and electromagnetic units (`abampere`, `statvolt`,
  `biot`, `franklin`, `gilbert`, `unit pole` and the rest), whose systems
  give charge different dimensions. The gauss, maxwell and oersted remain,
  because their SI equivalents are unambiguous and they are still in use.
- Variants defined at a historical reference temperature: the mean, 39 °F,
  59 °F and 60 °F BTUs, the mean, 15 °C and 20 °C calories, the perm, and
  mercury and water columns at a stated temperature.
- Units without a fixed definition: the boiler and water horsepowers and the
  clo.
- Duplicates of `std.units` units under another name, such as the metric ton
  and kilopond.

## Checks

Tests compare every catalogue unit, and the named units of `std.units`, with
the factor SP 811 table B.8 prints: within half a unit in its last digit for a
rounded factor, and within four ulps for an exact one. Another test checks
that each prefix exists on each SI unit with the right value and dimension.

## Acknowledgements

- Units and exact factors are from
  [NIST SP 811](https://www.nist.gov/pml/special-publication-811) (2008),
  appendix B, a US government work, and the
  [SI Brochure](https://www.bipm.org/en/publications/si-brochure) (9th
  edition); the ronna, quetta, ronto and quecto prefixes were added by the
  27th CGPM (2022).
- [Frink](https://frinklang.org/)'s unit file served as a checklist; no text
  was taken from it.

# std.units

`import std.units as u` provides the SI base dimensions, named derived
dimensions, units, physical constants and functions over the built-in
[`Quantity[T, D]`](../language/03-types/quantities.md). The module is ordinary
embedded Plenty: dimensions are `pub dimension` declarations and units are
`pub const` quantities. Nothing in it allocates.

```plenty
import std.units as u
from std.units import Velocity, km, hour, m, s

def main() -> Result[(), IoError | AllocError]:
    speed: Quantity[f64, Velocity] = 90.0 * km / hour
    braking = 6.0 * m / (s * s)
    distance = speed * speed / (2.0 * braking)
    print(speed, speed.to(u.knot))?
    print(f"{distance.to(m):.1f} m, {(speed / braking).to(s):.2f} s"?)?
    Ok(())
```
```output
25.0 m/s 48.59611231101511
52.1 m, 4.17 s
```

Every unit is a `Quantity[f64, D]` holding its size in the coherent SI unit,
so `km` stores `1000.0`. Construction multiplies by the unit and `to` divides
by it; both are folded at compile time when the operands are constants. A
conversion between two non-coherent units divides two rounded values, so it
can round: `u.foot.to(u.inch)` is `12.000000000000002`. The stored values
themselves are the nearest `f64` to each exact definition.

Printing shows the coherent value and SI symbols. Symbols are ordered by base
dimension name, so a newton prints as `m*kg/s^2`; see
[formatting](../language/03-types/quantities.md#formatting).

## Dimensions

| Base | Coherent unit |
|---|---|
| `Mass` | `kg` |
| `Length` | `m` |
| `Time` | `s` |
| `Current` | `A` |
| `Temperature` | `K` |
| `Amount` | `mol` |
| `Luminosity` | `cd` |

Derived dimensions are aliases; each names a different dimension, so
diagnostics can name it:

| Group | Dimensions |
|---|---|
| Geometry and motion | `Area`, `Volume`, `Wavenumber`, `Velocity`, `Acceleration`, `Frequency`, `Momentum` |
| Mechanics | `Force`, `Pressure`, `Energy`, `Power`, `Action`, `Density`, `SurfaceTension`, `Gravitation` |
| Flow | `MassFlowRate`, `VolumeFlowRate`, `DynamicViscosity`, `KinematicViscosity` |
| Heat | `HeatFlux`, `SpecificEnergy`, `Entropy`, `SpecificHeatCapacity`, `ThermalConductivity`, `StefanBoltzmann` |
| Chemistry | `MolarMass`, `MolarEnergy`, `MolarHeatCapacity`, `Concentration`, `PerAmount`, `CatalyticActivity` |
| Electromagnetism | `Charge`, `Voltage`, `ElectricField`, `Capacitance`, `Permittivity`, `Resistance`, `Conductance`, `MagneticFlux`, `MagneticFluxDensity`, `Inductance`, `Permeability` |
| Light | `Illuminance`, `LuminousEfficacy` |

SI gives some different kinds of quantity one dimension. `Energy` also
describes torque, `Frequency` also describes activity in becquerels, and
`SpecificEnergy` also describes absorbed and equivalent dose. A program that
needs to keep such kinds apart declares its own base dimension.

## Units

SI units, their prefixed forms, `eV` and `Wh` use their symbols. Other units
use names. Micro is written `u`: `um`, `us`, `uF`.

| Dimension | Units |
|---|---|
| `Mass` | `ug`, `mg`, `g`, `kg`, `tonne`, `dalton`, `pound`, `ounce` |
| `Length` | `pm`, `nm`, `um`, `mm`, `cm`, `m`, `km`, `inch`, `foot`, `yard`, `mile`, `nautical_mile`, `au`, `light_year`, `parsec` |
| `Time` | `ps`, `ns`, `us`, `ms`, `s`, `minute`, `hour`, `day` |
| `Current` | `uA`, `mA`, `A`, `kA` |
| `Temperature` | `mK`, `K`, `rankine` |
| `Amount` | `umol`, `mmol`, `mol`, `kmol` |
| `Luminosity` | `cd`, `lm` |
| `One` | `rad`, `sr`, `degree`, `arcminute`, `arcsecond` |
| `Area`, `Volume` | `hectare`, `acre`; `litre`, `millilitre`, `gallon` (US liquid) |
| `Velocity` | `knot` |
| `Frequency` | `Hz`, `kHz`, `MHz`, `GHz`, `Bq` |
| `Force` | `mN`, `N`, `kN`, `MN`, `pound_force` |
| `Pressure` | `Pa`, `hPa`, `kPa`, `MPa`, `GPa`, `mbar`, `bar`, `atm`, `torr`, `mmHg`, `psi` |
| `Energy` | `mJ`, `J`, `kJ`, `MJ`, `GJ`, `Wh`, `kWh`, `MWh`, `GWh`, `eV`, `keV`, `MeV`, `GeV`, `calorie`, `kilocalorie`, `btu` |
| `Power` | `uW`, `mW`, `W`, `kW`, `MW`, `GW`, `horsepower` |
| `Charge` | `nC`, `uC`, `mC`, `C` |
| `Voltage` | `uV`, `mV`, `V`, `kV`, `MV` |
| `Capacitance` | `pF`, `nF`, `uF`, `mF`, `F` |
| `Resistance`, `Conductance` | `ohm`, `kohm`, `Mohm`; `uS`, `mS`, `S` |
| Magnetism | `Wb`; `uT`, `mT`, `T`; `nH`, `uH`, `mH`, `H` |
| Other | `lx`, `Gy`, `Sv`, `kat` |

There is no milliohm constant, because `mohm` could be read as milli or mega;
write `1e-3 * u.ohm`. Angles are dimensionless, as in SI, so `u.degree` is
`Quantity[f64, One]` holding π/180. `calorie` is the thermochemical calorie
(4.184 J), `btu` the International Table BTU, `horsepower` the mechanical
horsepower (550 ft·lbf/s) and `mmHg` the conventional millimetre of mercury.
Celsius and Fahrenheit are not units here; see
[temperature](#temperature-and-time).

## Constants

| Constant | Value | Status |
|---|---|---|
| `delta_nu_Cs` | 9192631770 Hz, caesium hyperfine frequency | Exact SI defining constant |
| `c` | 299792458 m/s, speed of light | Exact SI defining constant |
| `h` | 6.62607015e-34 J·s, Planck constant | Exact SI defining constant |
| `e` | 1.602176634e-19 C, elementary charge | Exact SI defining constant |
| `k_B` | 1.380649e-23 J/K, Boltzmann constant | Exact SI defining constant |
| `N_A` | 6.02214076e23 /mol, Avogadro constant | Exact SI defining constant |
| `K_cd` | 683 lm/W, luminous efficacy | Exact SI defining constant |
| `hbar` | `h / (2π)` | Exact, rounded to `f64` |
| `R` | `N_A * k_B`, molar gas constant | Exact, rounded to `f64` |
| `sigma` | 5.670374419e-8 W/(m²·K⁴), Stefan–Boltzmann constant | Exact value truncated as CODATA lists it |
| `g_n` | 9.80665 m/s², standard gravity | Exact by definition |
| `atm` | 101325 Pa, standard atmosphere | Exact by definition |
| `G` | 6.67430e-11 m³/(kg·s²), gravitational constant | CODATA 2022 |
| `m_e`, `m_p`, `m_u` | Electron, proton and atomic mass constant | CODATA 2022 |
| `epsilon_0`, `mu_0` | Vacuum permittivity and permeability | CODATA 2022 |

## Functions

| Function | Result | Contract |
|---|---|---|
| `abs[Num, D](x)` | `Quantity[Num, D]` | `std.math.abs` of the stored value; any storage |
| `clamp[Num, D](x, low, high)` | `Quantity[Num, D]` | `math.min(math.max(x, low), high)`, so `low > high` gives `high`; any storage |
| `sum[D](items: &list[Quantity[f64, D]])` | `Quantity[f64, D]` | Neumaier compensated sum; an empty list gives zero. `math.sum(&items, 0.0 * u.m)` adds without compensation |
| `mean[D](items: &list[Quantity[f64, D]])` | `Option[Quantity[f64, D]]` | `sum(items) / len(items)`; `Nothing` for an empty list |
| `sqrt[D](x: Quantity[f64, D])` | `Result[Quantity[f64, Root[D, 2]], MathError]` | `std.math.sqrt`, with its errors |
| `cbrt[D](x: Quantity[f64, D])` | `Result[Quantity[f64, Root[D, 3]], MathError]` | `std.math.cbrt`, with its errors |
| `hypot[D](a, b)` | `Quantity[f64, D]` | `sqrt(a*a + b*b)` scaled to avoid intermediate overflow; infinity if either side is infinite |
| `exp`, `log`, `log2`, `log10` | `Result[Quantity[f64, One], MathError]` | Take `Quantity[f64, One]`; `std.math` errors |
| `sin`, `cos`, `tan` | `Result[Quantity[f64, One], MathError]` | Take an angle as `Quantity[f64, One]` in radians |
| `asin`, `acos`, `atan` | `Result[Quantity[f64, One], MathError]` | Return an angle in radians |
| `atan2[D](y, x)` | `Result[Quantity[f64, One], MathError]` | Both arguments have dimension `D`; the angle of `(x, y)` |

Every argument is a copied quantity, except `sum` and `mean`, which borrow the
list. Functions built on `std.math` take `f64` storage, as `std.math` does.
`abs` and `clamp` accept integer and domain storage too. The generic
[`std.math`](math.md) functions `min`, `max`, `min_all`, `max_all`, `sum` and
`prod` already accept quantities, so this module does not repeat them;
`math.abs` does not, because it compares with a plain zero. A square or cube root whose dimension is not
whole is a compile error at the call that needs it:

```plenty-error
import std.units as u
import std.math as math

def main() -> Result[(), math.MathError]:
    side = u.sqrt(2.0 * u.litre)?
    Ok(())
```
```error
`Root[Pow[std.units.Length, 3], 2]`: not a whole dimension
```

A root derives its dimension, so a formula's result is checked. Falling
through a tunnel across a uniform-density Earth is simple harmonic motion: the
trip to the centre takes a quarter period, `(π/2)·sqrt(R/g)`, and the speed
there is `sqrt(g·R)`:

```plenty
import std.units as u
from std.units import km, m, s, minute
import std.math as math

def main() -> Result[(), IoError | AllocError | math.MathError]:
    radius = 6371.0 * km
    to_centre = f64.PI / 2.0 * u.sqrt(radius / u.g_n)?
    top_speed = u.sqrt(u.g_n * radius)?
    print(f"{to_centre.to(minute):.1f} min, {top_speed.to(km / s):.2f} km/s"?)?
    Ok(())
```
```output
21.1 min, 7.90 km/s
```

Dimensionless results stay quantities, so an angle or a ratio combines with
other quantities without `.get()`:

```plenty
import std.units as u
from std.units import m
import std.math as math

def main() -> Result[(), IoError | AllocError | math.MathError]:
    rise = 3.0 * m
    run = 4.0 * m
    angle = u.atan2(rise, run)?
    slope = u.hypot(rise, run)
    print(f"{angle.to(u.degree):.2f} degrees, {slope}"?)?
    print(slope * u.cos(angle)?, u.sqrt(rise * run)?)?
    lengths = [rise, run, slope]?
    print(u.sum(&lengths), u.mean(&lengths))?
    Ok(())
```
```output
36.87 degrees, 5.0 m
4.0 m 3.4641016151377544 m
12.0 m Option[Quantity[f64, std.units.Length]].Some(Quantity[f64, std.units.Length](4.0))
```

## Temperature and time

| Function | Result | Contract |
|---|---|---|
| `from_celsius(degrees: f64)` | `Quantity[f64, Temperature]` | `degrees + 273.15` kelvin |
| `to_celsius(t)` | `f64` | `t` in kelvin minus `273.15` |
| `from_fahrenheit(degrees: f64)` | `Quantity[f64, Temperature]` | Through Celsius: `(degrees - 32) * 5/9 + 273.15` |
| `to_fahrenheit(t)` | `f64` | Through Celsius: `(t - 273.15) * 9/5 + 32` |
| `from_duration(span: &Duration)` | `Quantity[f64, Time]` | Seconds in `f64` |
| `to_duration(t: Quantity[f64, Time])` | `Result[Duration, RangeError]` | Rounds to the nearest nanosecond; `RangeError.OutOfRange` for a negative, NaN or too-large time |

Celsius and Fahrenheit have offset zeros, so they are not multiplicative units.
They convert at the boundary to an absolute temperature; a temperature
difference in degrees Celsius has the same size as one in kelvin. Going
through Celsius keeps whole Celsius values exact: `from_fahrenheit(212.0)` is
`373.15 K`.

A [`std.time.Duration`](time.md) counts nanoseconds as `u64`. `from_duration`
converts whole seconds and the remainder separately, but the result is still
an `f64`: spans longer than about 104 days (2^53 nanoseconds) no longer resolve
single nanoseconds.

```plenty
import std.units as u
import std.time as time

def main() -> Result[(), IoError | RangeError]:
    boil = u.from_fahrenheit(212.0)
    print(boil, u.to_celsius(boil), u.to_fahrenheit(u.from_celsius(-40.0)))?
    timeout = u.to_duration(1.5 * u.minute)?
    print(timeout.as_seconds(), u.from_duration(&time.milliseconds(250)))?
    print(u.to_duration(-1.0 * u.s).is_err())?
    Ok(())
```
```output
373.15 K 100.0 -40.0
90 0.25 s
True
```

## Limits

- Units and functions use `f64` storage. A quantity with `f32` storage
  converts with `Quantity[f32, D].from(q)`.
- There are no list conversions between `list[f64]` and
  `list[Quantity[f64, D]]` yet; they need compiler support to avoid copying.
  See the [backlog](../../backlog.md#units-of-measure).
- Named units are not printed: a joule prints as `m^2*kg/s^2`.
- Generic functions that differ only in dimension compile to separate bodies.

## Acknowledgements

- Exact unit definitions are from the
  [SI Brochure](https://www.bipm.org/en/publications/si-brochure) (9th edition)
  and [NIST SP 811](https://www.nist.gov/pml/special-publication-811),
  appendix B.
- Measured constants are the
  [CODATA 2022](https://physics.nist.gov/cuu/Constants/) recommended values.
- [misu](https://github.com/cjrh/misu) and [Frink](https://frinklang.org/)
  informed the choice of units; Frink's samples suggested the tunnel example.

# std.stat

`import std.stat` supplies descriptive statistics over `&list[f64]` and
normal, uniform and exponential distributions. It is checked Plenty code;
there is no native statistics dependency or global random state.

```plenty
import std.random
import std.stat
from std.stat import StatError

def main() -> Result[(), AllocError | IoError | StatError]:
    values = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]?
    print(std.stat.mean(&values)?, std.stat.median(&values)?, std.stat.pstdev(&values)?)?
    print(std.stat.quantiles(&values)?)?
    scores = std.stat.normal(100.0, 15.0)?
    print(scores.cdf(100.0)?, scores.inverse_cdf(0.5)?)?
    mut rng = std.random.seeded(42u64)
    draw = scores.sample(&mut rng)?
    print(draw > 0.0 and draw < 200.0)?
    Ok(())
```
```output
5.0 4.5 2.0
[4.0, 4.5, 5.5]
0.5 100.0
True
```

## Observations

All observations must be finite. Empty data returns `StatError.Empty`, and
NaN or infinity returns `NonFinite`. Convert integer observations explicitly
to `f64`; integers beyond 2^53 may lose precision during conversion.

| Function | Meaning | Allocation |
|---|---|---|
| `mean(values)` | Arithmetic mean | None |
| `pvariance(values)`, `pstdev(values)` | Population variance/deviation, denominator `n` | None |
| `variance(values)`, `stdev(values)` | Sample variance/deviation, denominator `n-1`; needs two observations | None |
| `median(values)` | Middle value, or midpoint of the middle pair | Copy and sort |
| `quantile(values, p)` | Inclusive linear interpolation, `0 <= p <= 1` | Copy and sort |
| `quantiles(values, n=4)` | The `n-1` cuts at `i/n`, `n >= 1` | Copy/sort plus output list |

Quantiles use Hyndman–Fan type 7: interpolate at sorted index `p*(length-1)`.
This corresponds to Python's `statistics.quantiles(method="inclusive")`,
**not its default exclusive method**. A singleton yields its one value at
every probability. `n=1` produces an empty list after validating the data.
The original list is never mutated. Sorting takes O(n log n) time and O(n)
additional space; the other functions take O(n) time and constant space.

Mean uses scaled, compensated summation. Deviation uses a corrected two-pass
calculation with scaled differences. These avoid many overflow/cancellation
problems of summing raw values or squares, but are still approximate f64
calculations, not exact arithmetic. Underflow may round to zero. An
unrepresentable result is `Overflow`; deviation can succeed when variance
overflows. A one-element sample returns `InsufficientData`.

## Distributions

Constructors validate parameters and return `Result[Distribution, StatError]`:

| Constructor | Parameters |
|---|---|
| `normal(mean=0.0, sigma=1.0)` | Finite mean and strictly positive finite standard deviation |
| `uniform(low=0.0, high=1.0)` | Finite ordered bounds; width and reciprocal width must be representable |
| `exponential(rate=1.0)` | Strictly positive finite rate |

All three provide `pdf(x)`, `cdf(x)`, and `inverse_cdf(p)`, returning
`Result[f64, StatError]`. NaN observations are rejected; infinite observations
have their usual limiting densities and probabilities. Probabilities must be
finite and in `[0,1]`. Normal inverse endpoints are negative/positive infinity;
exponential's upper endpoint is infinity. Uniform inverse endpoints are its
bounds, and its density includes both bounds (irrelevant to integrals).
Parameters are private and cannot be changed after construction.

Normal tails use a central power series and Laplace's continued fraction for
the Mills ratio; inversion bisects the smaller tail. Regression checks cover
central values, tails through probability 1e-100 and inverse round trips with
a 2e-13 relative tolerance. This is a tested tolerance, not a correctly-rounded
guarantee for every f64. Upper-tail CDFs eventually round to one; lower tails
underflow near 39 standard deviations. Inversion uses 64 iterations of the
bounded tail evaluation and is intended for ordinary statistical use, not
high-throughput vector analysis. Exponential uses `expm1`/`log1p` to retain
small-probability precision. Finite-result arithmetic overflow is reported.

## Sampling and reproducibility

Pass a mutable [`std.random.Rng`](random.md) to `sample`. Uniform returns
`f64`; normal and exponential return `Result[f64, StatError]` because the
transformed value can overflow. An error leaves the generator unchanged.
All constructors, queries and sampling operations allocate nothing.

| Distribution | Draw policy |
|---|---|
| Uniform | One `next_f64()` draw `u`; interpolate between bounds |
| Exponential | One draw `u`; `-log1p(-u)/rate` |
| Normal | Two draws `u,v`; Box–Muller `mean + sigma*sqrt(-2*log1p(-u))*cos(tau*v)`; no cached partner |

The draw counts and these transformations are version-stable; adding
distributions does not change any `std.random` sequence. Transcendental
functions follow the platform math implementation, so samples are reproducible
within the same math environment, not promised bit-identical across platforms
or libm updates. Rounding can put a uniform sample exactly at the upper bound.
No sampling function reads the clock, OS entropy, or a global generator.

## Errors and scope

`StatError` has `Empty`, `InsufficientData`, `NonFinite`, `Domain`, `Overflow`
and `Allocation(AllocError)`. Invalid distribution parameters, probabilities
and quantile counts are `Domain`; failures to allocate sorted/output lists
retain the original allocation cause. These operations have no hidden retry.

This module does not provide hypothesis tests, regression, weighted statistics,
discrete distributions, arbitrary precision or dataframe aggregation. Use
[`std.dataframe`](dataframe.md) for bulk analytical queries.

Algorithm references: [Python statistics](https://docs.python.org/3/library/statistics.html),
[statrs distributions](https://docs.rs/statrs/latest/statrs/distribution/index.html),
and [Lee's analysis of the Laplace fraction](https://www.ism.ac.jp/editsec/aism/pdf/044_1_0107.pdf).
See the [statistics tutorial](../../tutorial/07-modules/22-statistics.md) for examples.

## Acknowledgements

- API modelled on [Python's `statistics`](https://docs.python.org/3/library/statistics.html); distributions follow [statrs](https://docs.rs/statrs/).
- Quantiles use type 7 of [Hyndman and Fan](https://doi.org/10.2307/2684934) (1996).
- Normal tails use Laplace's continued fraction, following [Lee's analysis](https://www.ism.ac.jp/editsec/aism/pdf/044_1_0107.pdf).

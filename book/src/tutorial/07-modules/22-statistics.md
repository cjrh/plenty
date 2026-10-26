# Summarize observations and model uncertainty

Use `std.stat` for small numerical datasets and probability calculations.
Choose a population statistic when the observations are the complete group
you want to describe; choose a sample statistic when estimating the spread
of a larger population from a sample.

```plenty
import std.stat as stat

def main() -> Result[(), AllocError | IoError | stat.StatError]:
    measurements = [4.0, 1.0, 3.0, 2.0]?
    print(stat.mean(&measurements)?, stat.median(&measurements)?)?
    print(stat.quantiles(&measurements)?)?
    print(measurements)?
    Ok(())
```
```output
2.5 2.5
[1.75, 2.5, 3.25]
[4.0, 1.0, 3.0, 2.0]
```

Median and quantiles copy and sort the input; mean and standard deviation
do not allocate. Quantiles interpolate between observed values with the
smallest and largest at probabilities zero and one. Inputs must be finite,
and sample variance needs at least two observations.

Distributions model probabilities. A uniform distribution over a two-to-six
second delay has accumulated one quarter of its probability by three seconds:

```plenty
import std.stat as stat
import std.random as random

def main() -> Result[(), IoError | stat.StatError]:
    delays = stat.uniform(2.0, 6.0)?
    print(delays.cdf(3.0)?, delays.inverse_cdf(0.25)?)?
    mut first = random.seeded(42u64)
    mut repeated = random.seeded(42u64)
    print(delays.sample(&mut first) == delays.sample(&mut repeated))?
    bell = stat.normal()?
    print(bell.cdf(0.0)?)?
    Ok(())
```
```output
0.25 3.0
True
0.5
```

`normal(mean, sigma)` and `exponential(rate)` also provide density, CDF,
inverse CDF and sampling. Their samples can overflow and therefore return
`Result`; failure leaves the supplied generator unchanged. Seed explicitly
to reproduce a simulation, and use `std.secrets` for security-sensitive
randomness. Normal/exponential calculations use platform math, so their last
bits may differ between math-library versions.

The [reference](../../design/stdlib/stat.md) describes numeric limits,
errors, costs and sampling's fixed draw counts.

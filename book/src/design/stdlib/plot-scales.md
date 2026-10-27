# Plot position scales

[`std.plot`](plot.md) trains shared axes across every layer's effective data
and mapping. The last scale component for each axis wins, including components
added with `plus`. Scale names supply axis titles; `labs` overrides them.

```plenty
import std.bytes
import std.io
import std.plot as gg
from std.bytes import BytesError
from std.plot import Column, PlotError

def main() -> Result[(), AllocError | IoError | PlotError | BytesError]:
    data = gg.table([
        ("requests", Column.Num([10.0, 100.0, 1000.0, 10000.0]?)),
        ("latency", Column.Num([2.0, 3.0, 5.0, 9.0]?)),
    ]?)?
    chart = gg.ggplot(data, gg.aes("requests", "latency"), [
        gg.geom_point(),
        gg.scale_x_log10(
            name="Requests per second",
            breaks=Some([10.0, 1000.0]?),
            labels=Some(["10", "1k"]?),
        ),
        gg.scale_y_continuous(name="Latency (ms)", limits=(0.0, 10.0)),
    ]?)?
    mut output = std.io.cursor(std.bytes.empty()?)
    drop(chart.write_svg(&mut output)?)
    svg = output.data.to_text()?
    print(">1k<" in svg, ">100<" in svg, "Latency (ms)" in svg)?
    Ok(())
```
```output
True False True
```

## Continuous axes

`scale_x_continuous` and `scale_y_continuous` accept `name`, `breaks`
(`Option[list[f64]]`), `labels` (`Option[list[str]]`), `limits=(low, high)`, and
`transform`. `Transform.Identity`, `Log10`, `Sqrt`, and `Reverse` are
supported. The `scale_{x,y}_log10`, `_sqrt`, and `_reverse` helpers select
those transforms directly.

Breaks and limits use original data units; ticks are positioned using the
transform and labelled in original units. Explicit labels require explicit
breaks of equal length. Breaks must be distinct, finite and in the transform
domain. Omit breaks (or pass `Nothing`) for automatic ticks; use
`breaks=Some([1.0, 10.0, 100.0]?)` for specific values and
`breaks=Some([]?)` to hide ticks. Supply labels as `Some([... ]?)`.
Limits must be finite, increasing and
inside the transform domain. Without breaks, ticks use 1/2/5 steps in
transformed space and automatic ranges have padding.

Explicit labels allow units, percentages, or date strings. Automatic calendar
breaks, date columns and formatting callbacks are not implemented.

## Discrete axes

`scale_x_discrete` and `scale_y_discrete` accept `name`, `limits` (an
optional ordered list of distinct level strings), and `labels`. Pass, for
example, `limits=Some(["South", "North"]?)`. Limits set category
order, retain named unused categories, and omit rows in unlisted categories.
Labels must match the resulting level list. Without limits, levels are sorted
as described under [mappings](plot.md#mappings-and-components); numeric factors
keep numeric ordering.

## Limits and transformations affect statistics

Continuous scale limits omit out-of-range input values **before** statistics.
Values outside a transform's domain (for example zero on a log axis) are
omitted too, with removal notes unless `na_rm=True`. Missing/omitted values
interrupt lines. Discrete limits similarly exclude unlisted categories.

Histograms bin transformed x values. Supplied bar heights are transformed before
stacking; computed counts are transformed before position adjustment.
Consequently log-transformed stacks sum logarithms, not original heights; use
unstacked bars when that is the intended interpretation. Bar baselines are zero
in transformed space (one in original units for log10).

`coord_cartesian` limits only zoom after these computations and use original
data units even with transformed scales. Use coordinate limits when statistics
must still include values outside the view. `coord_flip` exchanges the displayed
axes after applying their scales; x scale settings still describe the x mapping.

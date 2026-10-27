# Compare panels and show ranges

Use facets to compare the same measurements across groups. Add error bars or
a ribbon when your data includes lower and upper bounds. The plot draws these
supplied values; calculating a confidence interval or another statistical
summary is a separate step.

This example uses illustrative measurement bounds, not fitted confidence
intervals:

```plenty
import std.plot as gg
import std.io
import std.bytes
from std.plot import Column, PlotError
from std.bytes import BytesError

def main() -> Result[(), AllocError | IoError | PlotError | BytesError]:
    data = gg.table([
        ("day", Column.Num([1.0, 2.0, 3.0, 1.0, 2.0, 3.0]?)),
        ("value", Column.Num([2.0, 4.0, 3.0, 3.0, 5.0, 4.0]?)),
        ("lower", Column.Num([1.0, 2.0, 1.5, 2.0, 3.0, 2.5]?)),
        ("upper", Column.Num([3.0, 6.0, 4.5, 4.0, 7.0, 5.5]?)),
        ("site", Column.Text(["North", "North", "North",
                             "South", "South", "South"]?)),
    ]?)?
    chart = gg.ggplot(data, gg.aes("day", "value", ymin="lower", ymax="upper"), [
        gg.geom_ribbon(fill="blue", alpha=0.2),
        gg.geom_errorbar(colour="navy", width=0.15),
        gg.geom_line(colour="navy"),
        gg.geom_point(colour="navy"),
        gg.facet_wrap("site", ncol=2),
        gg.labs(title="Measurement ranges by site", x="Day", y="Value"),
        gg.theme_minimal(),
    ]?)?
    mut output = std.io.cursor(std.bytes.empty()?)
    notes = chart.write_svg(&mut output, width=10.0, height=4.0)?
    print(notes, output.data.to_text()?.startswith("<svg"))?
    Ok(())
```
```output
[] True
```

Fixed axes let you compare heights directly between sites. Choose
`scales=gg.Scales.FreeY` when each panel needs its own vertical range, but then
compare the axis values rather than the visual heights. With two grouping
variables, use `gg.facet_grid(rows="region", cols="period")`.

A reference layer whose table omits `site` appears in every panel. Give it its
own mapping with `inherit_aes=False` when it should not inherit aesthetics
whose columns are absent; see [combine plot layers](27-plot-layers.md).
Histogram counts and bar stacks remain independent within each panel.

Both interval endpoints contribute to the axes. Missing bounds omit an error
bar and interrupt a ribbon at that x; inspect the returned notes with real data.
`geom_linerange` draws the same interval without caps. To display horizontal
intervals, add `coord_flip`.

The larger output size gives two panels room for their labels. All panels are
checked before writing SVG, so a chart that cannot fit reports an error instead
of hiding labels. See [facets](../../design/stdlib/plot-facets.md) and
[uncertainty and intervals](../../design/stdlib/plot-intervals.md) for the full
contracts and current limits.

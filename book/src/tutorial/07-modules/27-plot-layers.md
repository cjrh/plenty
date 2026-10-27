# Combine plot layers and control axes

A summary often has different columns and fewer rows than its observations.
Give that layer its own table and mapping. Use `inherit_aes=False` when it
should not inherit grouping or other aesthetics from the observations.

```plenty
import std.plot as gg
import std.io
import std.bytes
from std.plot import Column, PlotError
from std.bytes import BytesError

def main() -> Result[(), AllocError | IoError | PlotError | BytesError]:
    observations = gg.table([
        ("dose", Column.Num([1.0, 10.0, 100.0]?)),
        ("response", Column.Num([2.0, 4.0, 8.0]?)),
        ("site", Column.Text(["North", "South", "North"]?)),
    ]?)?
    reference = gg.table([
        ("dose", Column.Num([1.0, 100.0]?)),
        ("expected", Column.Num([2.0, 7.0]?)),
    ]?)?
    chart = gg.ggplot(observations, gg.aes("dose", "response", colour="site"), [
        gg.geom_point(size=2),
        gg.geom_line(data=reference, mapping=gg.aes("dose", "expected"),
                     inherit_aes=False, colour="navy"),
        gg.scale_x_log10(breaks=Some([1.0, 10.0, 100.0]?),
                         labels=Some(["1 mg", "10 mg", "100 mg"]?)),
        gg.labs(title="Observed and expected response"),
        gg.theme_minimal(),
    ]?)?
    mut output = std.io.cursor(std.bytes.empty()?)
    notes = chart.write_svg(&mut output)?
    print(notes, output.data.to_text()?.startswith("<svg"))?
    Ok(())
```
```output
[] True
```

The line inherits no site grouping, so it connects the two reference points.
Both layers train the same axes. The logarithmic x scale gives equal horizontal
spacing to each tenfold increase, while breaks and labels use original doses.
The explicit labels include units without changing the data.

To override only one mapping while keeping the others, use a layer argument
such as `mapping=gg.aes(y="expected")` with the default `inherit_aes=True`.
Every inherited name must exist in that layer's table.

For a categorical axis, `gg.scale_x_discrete(limits=Some(["South", "North"]?))`
sets the display order and excludes any unlisted categories. Its `labels`
argument can supply display names in the same order. It requires a discrete
x mapping; the dose example uses a continuous one.

Use `coord_cartesian(xlim=(1.0, 10.0))` to zoom while retaining all rows for
statistics. A scale's `limits` instead removes values before computation, which
can change histogram counts. Values outside a transformation's domain are
also omitted and reported in the renderer's notes. Check these notes when
working with real input.

Long titles and labels wrap automatically. The renderer reserves space for
their text boxes and checks for overlapping ticks before writing SVG. If it
reports that the chart cannot fit, increase `width` or `height`, shorten the
labels, or provide fewer breaks. For example, a presentation chart can use
`chart.write_svg(&mut output, width=10.0, height=6.0)?`. Labels are never
silently dropped to make the chart fit.

See [position scales](../../design/stdlib/plot-scales.md) for the full contracts,
including square-root and reversed axes, and [draw a chart](26-plotting.md)
for file output and record accessors.

# Draw a chart

[`std.plot`](../../design/stdlib/plot.md) turns data into SVG without an external
plotting program. Its names follow ggplot2: a plot combines a table, mappings
from column names to visual properties, and a list of layers and styling choices.

This example renders into a memory stream so the lesson creates no files:

```plenty
import std.plot as gg
import std.io
import std.bytes
from std.plot import Column, PlotError
from std.bytes import BytesError

def main() -> Result[(), AllocError | IoError | PlotError | BytesError]:
    data = gg.table([
        ("hour", Column.Num([9.0, 12.0, 15.0]?)),
        ("temperature", Column.Num([16.0, 22.0, 20.0]?)),
    ]?)?
    chart = gg.ggplot(data, gg.aes("hour", "temperature"), [
        gg.geom_line(colour="navy"),
        gg.geom_point(colour="navy", size=2),
        gg.labs(title="Afternoon temperatures", x="Hour", y="Degrees C"),
        gg.theme_minimal(),
    ]?)?
    mut output = std.io.cursor(std.bytes.empty()?)
    notes = chart.write_svg(&mut output)?
    print(notes)?
    print(output.data.to_text()?.startswith("<svg"))?
    Ok(())
```
```output
[]
True
```

For a file, replace the cursor and `write_svg` with
`notes = chart.save("temperatures.svg")?`. Open the file in a browser or include
it in a document. Saving borrows the chart, so you can save it again at another
size. The default is 6 by 4 inches; named `width`, `height`, and `units` control
dimensions. Only SVG is supported currently.

The distinction between **mapping** and **setting** matters. `aes(colour="kind")`
looks up a column and creates a colour scale and legend. `geom_point(colour="navy")`
uses one constant colour. Unknown column names fail when the plot is constructed.

## Plot your own records

Typed accessors turn lists of ordinary classes into columns. Compute derived
values in these accessors, where Plenty can type-check them; `aes` does not
evaluate strings as expressions.

```plenty
import std.plot as gg
import std.io
import std.bytes
from std.plot import PlotError
from std.bytes import BytesError

class Sale:
    region: str
    amount: f64

def main() -> Result[(), AllocError | IoError | PlotError | BytesError]:
    sales = [Sale("North", 12.0), Sale("South", 18.0), Sale("North", 5.0)]?
    data = gg.rows(&sales, [
        gg.text("region", def(s: &Sale) -> str: s.region)?,
        gg.num("amount", def(s: &Sale) -> f64: s.amount)?,
    ]?)?
    base = gg.ggplot(data, gg.aes("region", "amount", fill="region"), [
        gg.geom_col(),
        gg.scale_fill_manual({"North": "navy", "South": "#D55E00"}?),
        gg.theme_minimal(),
    ]?)?
    labelled = base.plus([gg.labs(title="Sales by region", y="Sales")]?)?
    mut output = std.io.cursor(std.bytes.empty()?)
    drop(labelled.write_svg(&mut output)?)
    print(len(sales), output.data.to_text()?.startswith("<svg"))?
    Ok(())
```
```output
3 True
```

`geom_col` uses supplied heights and stacks rows at the same x by default. Use
`geom_bar` with no y mapping to count rows instead, or `geom_histogram(bins=20)`
with a continuous x mapping to count numeric intervals. For grouped bars,
`position=gg.position_dodge()` places groups side by side.

The named palette keeps each region's colour stable even if the input order
changes. Include every observed region; extra names are allowed. For a numeric
colour mapping, use `gg.scale_colour_gradient(low="#132B43", high="#56B1F7")`
to choose endpoints instead. The legend follows the chosen colours. See
[manual palettes and gradients](../../design/stdlib/plot.md#manual-palettes-and-gradients)
for interpolation and validation rules.

`plus` leaves the original plot unchanged and shares its immutable data. `rows`
copies selected values, so the original records also remain usable. For columns
you already own, `table` moves the lists without copying them.

Missing and non-finite mapped values are omitted, with a note returned by the
renderer. Check these notes when interpreting a chart. The final example
discards them deliberately because its small, fixed dataset contains no missing
values. File/stream errors and allocation failures still propagate through `?`.
See the [reference](../../design/stdlib/plot.md) for supported layers, layout
limits, and differences from the larger plotting proposal.

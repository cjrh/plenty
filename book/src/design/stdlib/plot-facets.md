# Plot facets

[`std.plot`](plot.md) can split a plot into labelled panels. Facets use column
names, not formulas.

- `facet_wrap("site", ncol=2)` wraps one facet variable across a rectangular
  layout. `nrow` and `ncol` default to zero (automatic). If both are supplied,
  their product must accommodate every panel.
- `facet_grid(rows="region", cols="period")` uses one variable per dimension.
  Either dimension can be omitted. Every row/column combination is retained,
  including combinations with no observations.

Levels sort using the mapping rules: text lexically, numbers numerically.
Missing/non-finite facet values form a separate panel labelled `(missing)`;
they are not discarded as missing aesthetics. A literal string `"(missing)"`
remains a distinct category. Headings show the variable name and its value.
The last facet component wins, including when added with `plus`.

## Layers and statistics

Facet columns are read from each layer's effective table, independently of
`aes`. At least one layer must contain all requested facet variables.
A layer missing a facet variable repeats across its levels. This makes a
reference line without a `site` column appear in every site panel. A partially
specified grid layer repeats only across the missing dimension.

Counts, histogram bins and bar positions are computed separately per panel.
Colour/fill scales train over all layers and panels and use one shared legend.
Position scales are trained before statistics; their final ranges include
panel-specific counts, stacks and interval bounds.

## Fixed and free axes

Both constructors accept `scales=Scales.Fixed` (the default), `Free`,
`FreeX`, or `FreeY`.

For wrapped facets, free axes train independently in each panel. In a grid,
x scales are shared within each column and y scales within each row, even
when free. This retains grid alignment. Explicit scale or coordinate limits
still apply to every panel. Discrete scale limits retain their explicit order.
Fixed histogram x scales use common bin boundaries; free x scales train their
bin ranges independently within each scale group.

`coord_flip` exchanges displayed axes after scale training. `FreeX` always
refers to the x mapping, so its displayed direction changes under flipping.

## Layout and limits

All panel layouts are preflighted before any SVG is written. Panel sizes are
equal, with common margins so plotting areas align. Axis titles and ticks repeat
in each panel. The plot title and legend appear once. Increase output dimensions
when labels or panels cannot fit; panels are not silently omitted.

There is a maximum of 1,000 panels. Multiple variables in one wrap/dimension,
custom labellers, explicit facet ordering, proportional panel space, margins
panels, and interior-axis suppression are not implemented. To choose an order,
use a numeric grouping column and provide its meaning in the surrounding report.

Rendering materialises panel tables with fallible allocations and shares text
values; selections containing every row share the original table. Broadcast
layers still repeat their prepared geometry. Memory and preparation
time therefore grow with the number of panels and repeated observations; this
is not a lazy dataframe view. SVG fragments are still streamed.

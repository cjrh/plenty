# `std.plot`

`std.plot` draws layered charts as deterministic SVG files or byte streams.
It is implemented in Plenty and needs no plotting runtime, R installation,
display server, or external shared library. Import it with `import std.plot as gg`.

This is the core/SVG portion of the [plotting proposal](../../proposals/std-plot.md),
not the complete first milestone or a drop-in replacement for ggplot2. See the
[plotting tutorial](../../tutorial/07-modules/26-plotting.md) for runnable examples.

## Data and ownership

`table(list[(str, Column)])` consumes named columns without copying their lists.
Columns have equal lengths and unique nonempty names; an empty table has height
zero.
`Column.Num`, `Int`, `Text`, and `Bool` contain lists of `f64`, `i64`, `str`,
and `bool`. `MaybeNum` and `MaybeText` contain lists of `Option[f64]` and
`Option[str]`. `Table.height()` and `Table.names()` inspect the table.

`Table.share()` gives another immutable owner of the same columns without
allocating. `ggplot` consumes a table. `Plot.plus(components)` borrows the
original plot and returns an independent specification, sharing its table and
copying its mappings and components. Both plots remain usable.

`columns(&dictionary)` copies a `dict[str, list[f64]]`, retaining its key order.
`rows(&records, fields)` evaluates typed accessors and builds columns. Construct
fields with `num`, `integer`, `text`, and `flag`; each takes a name and an owned
callback such as `def(row: &Reading) -> f64: row.temperature`. These constructors
allocate a boxed callback. `rows` consumes the field list and retains no callbacks
or borrowed records. This also supports lists of tuples and derived columns.

## Mappings and components

`aes(x, y, colour=, color=, fill=, group=)` maps column **names**, not expressions.
`colour` and `color` are aliases; supplying both nonempty names is an error.
An empty name leaves that mapping unset. Numeric columns are continuous; text
and bool columns are discrete. `factor("name")` makes a numeric column discrete.
Numeric factors sort by numeric value; other levels sort by their string value.
`Int` positions and continuous colours convert to `f64`, with its usual precision
limit; factor labels preserve exact integers.

`ggplot(table, mapping, components)` eagerly checks names, required aesthetics,
column kinds, and component settings. A mapping is shared by every layer in this
version. Constants belong in a geom, such as `geom_point(colour="red")`; a colour
name inside `aes` would instead be looked up as a column.

| Constructor | Behavior |
| --- | --- |
| `geom_point` | Points; requires x and y; `size` is a diameter in millimetres |
| `geom_line` | Connects points sorted by x within each group |
| `geom_path` | Connects points in input order within each group |
| `geom_col` | Bars with supplied continuous y heights |
| `geom_bar` | Counts rows at each x, per group; omit y |
| `geom_histogram` | Counts continuous x in equal-width bins; omit y |

Each geom accepts `colour`/`color`, `alpha`, and `na_rm`; bars also accept `fill`
and `position`. Lines use `linewidth`. Numeric settings accept integer or float
arguments. Supported constant colours are black, white, red, green, blue, grey,
gray, orange, purple, yellow, pink, brown, navy, transparent, and `#RRGGBB` or
`#RRGGBBAA`. More R colour names are not yet supported.

Discrete colour/fill mappings default to an eight-colour palette, repeated for
additional levels. Continuous mappings default to an RGB gradient from `#132B43`
to `#56B1F7`. Legends are automatic. This is not yet ggplot2's variable-size HCL
palette or its continuous colour-bar guide.

### Manual palettes and gradients

`scale_colour_manual(values)` and `scale_fill_manual(values)` consume a
`dict[str, str]` mapping level labels to colours. For example,
`gg.scale_fill_manual({"North": "navy", "South": "#D55E00"}?)` keeps each
region's colour stable when rows are reordered or a region is absent.
Dictionary order does not change the sorted legend order. Every observed level
must have an entry; extra entries are allowed. Numeric categories need `factor`
and keys matching their displayed labels. Invalid colours, missing entries, and
continuous mappings return errors from `ggplot` or `plus`.

`scale_colour_gradient(low="#132B43", high="#56B1F7")` and
`scale_fill_gradient(...)` set continuous endpoints. Both endpoints must be
six-digit `#RRGGBB` colours, with either hex letter case. Interpolation is in
encoded RGB channels, truncating to integer channels; it is **not** ggplot2's
Lab interpolation. The finite data minimum and maximum map to the endpoints;
a constant column maps to the midpoint. Legend swatches use the same mapping.
Discrete data with a gradient returns `WrongKind`.

The `scale_color_manual` and `scale_color_gradient` spellings are aliases.
The last scale component for each aesthetic wins, regardless of its position
relative to geoms. `plus` can replace a scale without changing the original
plot. Colour and fill are independent; different palettes get separate legends
even when they map the same column. Constant geom colours override mappings.
Missing values are still omitted, rather than given a palette colour. Scale
limits, custom breaks, `na_value`, and palette functions are not yet exposed.

Default groups follow the interaction of discrete mappings; an explicit `group`
replaces this rule. Bar positions use colour/fill groups independently of x.
`position_identity`, `position_stack`, `position_fill`, and `position_dodge`
select unadjusted, stacked, proportion, and side-by-side bars. Positive and
negative stacks accumulate separately. Stacks follow reverse group order,
matching the top-to-bottom discrete legend. Dodge reserves one slot for every
group, including when a group is absent at a particular x. Bar `width` is in
data units (default 0.9); automatic numeric resolution-based widths are deferred.

Histogram `bins` defaults to 30 and may be 1–10,000. Bins span the finite x range;
they are left-closed/right-open, except that the final bin includes the maximum.
An identical-value input uses a unit-width bin centred on that value. Empty bins
do not produce rectangles. This deliberately small binning API does not yet
implement ggplot2's boundary/centre/binwidth alignment or computed statistics.
Count/bin layers require discrete colour/fill mappings. If an explicit group
combines different mapped colours in one output bar, rendering reports a conflict
instead of selecting one arbitrarily.

Rows with missing or non-finite mapped values are omitted and counted in returned
notes; `na_rm=True` suppresses the note. A missing y interrupts a line/path
within its group. Numeric data magnitudes above `1e100` are rejected at rendering.

## Labels, coordinates, and themes

`labs(title=, x=, y=, colour=, fill=)` overrides nonempty labels; default axis and
legend labels come from column names. Later nonempty label settings win.

`theme_grey`, `theme_minimal`, and `theme_classic` select grey-panel, white-grid,
and plain-axis styles. They accept `base_size` in points and `legend_position`
(`Legend.Right` or `Legend.Hidden`). A later theme replaces the earlier theme.
These are compact styles, not the proposal's complete theme-element tree.

`coord_flip()` exchanges the displayed axes. `coord_cartesian(xlim=(low, high),
ylim=(low, high))` clips and zooms **after** counting and stacking, preserving
statistics. Omit either pair to use its automatic range. Only one coordinate
component is allowed. Position scales are linear; ticks use 1/2/5 steps, and
automatic ranges have padding. Custom position scales and transforms are not yet exposed.

Layout currently reserves fixed margins and a right-hand legend area. SVG text
names `Liberation Sans, Arial, Helvetica, sans-serif`; no fonts or metrics ship.
Long labels can overlap or extend outside the page. Increase dimensions, shorten
labels, or hide the legend. A legend with too many rows for the height returns
`InvalidSetting` instead of silently omitting entries. Bundled font measurement,
text shaping, and sophisticated guide layout are not implemented.

## Output and failures

`plot.save("chart.svg", width=6.0, height=4.0, units=Units.In, dpi=300.0)`
writes SVG and returns `Result[list[str], PlotError]`. `write_svg(&mut writer, ...)`
accepts any [`std.io.Writer`](io.md) and also returns notes. Both borrow the plot,
so it can be rendered repeatedly. `Units.Cm`, `Mm`, and `Px` are also supported;
`dpi` converts pixel dimensions to physical dimensions and never rasterises SVG.
Dimensions must be between 2 and 100 inches on each axis.

Rendering prepares per-layer rows, writes SVG fragments directly to the writer,
and flushes it. `save` uses a 16 KiB file buffer. It does not collect the complete
SVG unless the caller selects a memory cursor. Output uses two-decimal coordinates, stable element order and
identifiers, and no timestamps or machine-specific font discovery. Identical
inputs with the same toolchain produce identical bytes.

`save` truncates an existing destination. A rendering, allocation, or I/O failure
can leave partial output; it is not an atomic file replacement. For transactional
publication, write a temporary file and rename it after success. A stream failure
is `PlotError.Write(completed, cause)`, where `completed` counts bytes in the
**failing fragment**, not the entire plot. Flush/close failures use `Io`.

Other errors are `UnknownColumn`, `DuplicateColumn`, `ShapeMismatch`,
`MissingAesthetic`, `WrongKind`, `Conflict`, `InvalidSetting`, `Allocation`,
`Bytes`, and `Format`. No library messages are printed. `ggplot` catches mapping
and setting mistakes; rendering can additionally reject unsupported numeric
magnification, XML-forbidden text, or insufficient layout space. Allocation
failure is always returned; no infallible native plotting allocator is involved.

## Current scope

Not yet implemented: layer-specific mappings/data, `after_stat`, statistical
smoothing, facets, position scales and other colour scales, full themes and font
metrics, PNG/PDF, dataframe input, or record reflection. These remain part of
the accepted proposal; priorities are tracked in [B80](../../backlog.md).

## Acknowledgements

- [ggplot2](https://ggplot2.tidyverse.org/) supplies the model for the public names,
  layered composition, aesthetic mappings, grouping, and stacked-bar semantics.
  Its source was consulted during implementation; no ggplot2 code or data ships.
- Hadley Wickham's [A Layered Grammar of Graphics](https://vita.had.co.nz/papers/layered-grammar.html)
  describes the data/mapping/layer/scale/coordinate separation used by the module.

# `std.plot`: a ggplot2-style plotting module

**Status: Partially implemented. Design accepted.** The
[core/SVG reference](../design/stdlib/plot.md) describes the implemented subset:
tables and record accessors, layer-specific data and inherited mappings, six
geoms, grouped counts/binning and bar positions, continuous/discrete position
scales with transforms and explicit breaks/labels, named manual palettes and
RGB gradients, basic themes/legends, and
streaming SVG. This does not complete milestone 1: the remaining scales,
facets, smoothing, measured layout and the full example
gallery remain future work, tracked under B80 in the [backlog](../backlog.md).
The [decisions](#decisions) remain the target design. Code fences here show
proposed API and are not compiled; use the reference and tutorial for current API.

## Problem

Plenty programs can compute summaries of data in lists, records, and
[`std.dataframe`](../design/stdlib/dataframe.md) frames, but cannot draw them.
Today a user writes CSV and opens R or Python to plot it.

The plotting library most data analysts know well is R's
[ggplot2](https://ggplot2.tidyverse.org/), built on Wickham's
[layered grammar of graphics](https://vita.had.co.nz/papers/layered-grammar.html)
(2010). A plot is data, plus aesthetic mappings from variables to visual
properties, plus layers (a geom, a stat, and a position adjustment), plus
scales, a coordinate system, facets, and a theme. Each part has defaults, so a
short specification gives a complete plot with axes and legends.

This proposal designs `std.plot`: the same grammar, with ggplot2's names and
defaults, expressed in ordinary Plenty. A ggplot2 user should be able to read a
`std.plot` program without documentation and write one after reading the
[correspondence table](#correspondence-with-ggplot2).

## Goals

- **The grammar, not only the drawing.** Mappings, layers with default stats,
  trained scales with automatic legends, facets, coordinates, and themes.
- **ggplot2 names and defaults.** `ggplot`, `aes`, `geom_point`,
  `scale_colour_brewer`, `facet_wrap`, `theme_minimal`, `labs`. Default theme,
  palette, bin count, units (points for text, millimetres for sizes) and
  layout match ggplot2 so values transfer and plots look familiar.
- **Plenty's builtin data.** Lists of records, dictionaries of columns, lists
  and tuples are first-class input. This is required, not an add-on.
- **`std.dataframe` later**, with the same plot API.
- **Plenty conventions.** Errors are `Result` values with precise causes,
  allocation is explicit and fallible, ownership is ordinary, and the output is
  byte-for-byte deterministic.
- **Written in Plenty**, following the
  [standard library policy](standard-library-scope.md#plenty-or-native), with
  one justified native dependency already in the toolchain.

## Non-goals

- Interactive or on-screen display. Output is files or `std.io` streams.
- R's non-standard evaluation: arbitrary expressions inside `aes()`.
- ggplot2's extension system (`ggproto`) and its ecosystem (`ggrepel`,
  `patchwork`, `gganimate`).
- Complex-script text shaping (Arabic, Indic scripts) in the first releases.
- Pixel-identical output to ggplot2. Layout and defaults match; renderers,
  fonts and smoothing details differ.

## Precedents

**ggplot2** composes with `+`: `ggplot(mpg, aes(displ, hwy)) + geom_point()`.
`+` dispatches on the right operand (`ggplot_add()`), so layers, scales,
themes and even lists of components can be added. `aes()` captures unevaluated
expressions (quosures) and evaluates them against the data frame at build time,
so `aes(colour = factor(cyl))` works without quoting. `after_stat()`,
`after_scale()` and `stage()` select when a mapping is evaluated. Version
4.0.0 (CRAN, 2025-09-11) moved to S7 classes and added theme-level
`ink`/`paper` colours and palette settings; the grammar is unchanged.

**[plotnine](https://plotnine.org/)** (0.15.8) ports ggplot2 to Python.
Python has operator overloading but no lazy evaluation, so plotnine keeps `+`
(`ggplot.__add__`) and replaces unevaluated expressions with strings: in
`aes("wt", "mpg", color="factor(cyl)")` each string names a column or is
evaluated as an expression against the dataframe, with columns taking
precedence over variables. It also accepts arrays and scalars as mapping
values. Theme elements use `legend_position` for `legend.position`. Rendering
goes through matplotlib. plotnine shows that strings are an acceptable
replacement for NSE for ggplot2 users; its expression evaluation is the part
that relies on Python's dynamic `eval`.

**[plotters](https://docs.rs/plotters/latest/plotters/)** (0.3.7) is a Rust
drawing library for charts. It is not related to the grammar of graphics: there
are no aesthetic mappings, scale training, stats, automatic legends from
mappings, or facets. A program sizes label areas in pixels, builds a Cartesian
coordinate range by hand, configures a mesh, and draws series (`LineSeries`,
point series, histograms) with explicit styles. It sits at the level of
ggplot2's `grid` graphics layer, roughly the bottom fifth of the work. Its
backends are SVG and a bitmap backend that pulls in `image`, and fonts through
`font-kit` (system font discovery) or `ab_glyph`. [Rendering](#rendering-and-output)
explains why it is not recommended as a backend.

## Language probes

Probes ran on local `main` at `dbcdf39a` with a debug compiler.
Since those probes, [arithmetic operator hooks](../design/language/04-expressions/operators.md)
have been implemented (issue #512). The historical rejection below no longer
applies; the composition recommendation still accounts for fallible allocation.

| Feature | Result | Consequence |
| --- | --- | --- |
| Operator `+` on a class with `__add__` | Rejected: ``operator `+` does not accept V`` | No `+` composition without a language change |
| Method with owned receiver `self: P` | Rejected: receivers are `&` or `&mut` | Builder methods cannot consume the plot |
| Free function consuming and returning a plot | Works | `add(add(p, a)?, b)?` is possible but reads inside out |
| Method borrowing `self` and returning a new value, chained on temporaries | Works: `base.plus(a)?.plus(b)?` | [Composition](#composition) uses this |
| Anonymous sums with defaults: `x: str \| AfterStat \| Unmapped = Unmapped.Unset` | Works; `"wt"` and `after_stat("count")` inject | `aes(x="wt", y=after_stat("count"))` needs no wrapper |
| `size: f64 \| Unset` given `size=2` | Rejected: `expected Unset \| f64, got i64` | Numeric settings accept `f64 \| i64` |
| Named arguments and defaults on functions and methods | Works | `geom_point(size=2, colour="red")` |
| Multi-line list display as an argument | Works | One component per line |
| Generic accessor `num("wt", def(c: &Car) -> f64: c.wt)` infers `T` | Works, via `BoxedCallable` | Records enter through typed accessors |
| `Car.wt` as a value | Rejected; the selector exists only for [`clist.column`](../design/facilities/01-builtins/clists.md) | No typed column handles yet |
| `copy()` of components holding closures | Not supported | The plot itself holds no closures |
| `Shared[T]` field with `share()` | Works; handles have no `copy()` | Plots share their table without copying it |

Also relevant: references cannot be stored in classes, so a plot cannot
borrow caller data; wildcard imports and re-exports do not exist; a function
may have at most 255 parameters; statements nest to 64 levels. The
[derived serialization proposal](derived-serialization.md) (B77, accepted,
not implemented) gives classes an `Encoder` hook that a plot table can consume.
[`std.compress`](../design/stdlib/compress.md) provides zlib compression and
CRC-32, which is all PNG needs beyond pixels.

## Correspondence with ggplot2

| ggplot2 | plotnine | `std.plot` |
| --- | --- | --- |
| `ggplot(df, aes(x, y))` | `ggplot(df, aes("x", "y"))` | `ggplot(table, aes("x", "y"))?` |
| `p + geom_point() + labs(...)` | same | `ggplot(table, mapping, [geom_point(), labs(...)]?)?` |
| `base + geom_line()` (base unchanged) | same | `base.plus([geom_line()]?)?` |
| `aes(colour = cyl)` | `aes(color="cyl")` | `aes(colour="cyl")` (`color=` also accepted) |
| `aes(colour = factor(cyl))` | `aes(color="factor(cyl)")` | `aes(colour=factor("cyl"))` |
| `aes(y = after_stat(density))` | `aes(y=after_stat("density"))` | `aes(y=after_stat("density"))` |
| `aes(y = 2 * hp)` | `aes(y="2*hp")` | Compute the column when building the table |
| `geom_point(colour = "red")` (setting) | same | `geom_point(colour="red")` |
| `geom_bar(position = "dodge")` | same | `geom_bar(position=position_dodge())` |
| `geom_smooth(method = "lm")` | same | `geom_smooth(method=Smooth.Lm)` |
| `geom_histogram(binwidth = 500)` | same | `geom_histogram(binwidth=500)` |
| `scale_x_log10()` | same | `scale_x_log10()` |
| `scale_colour_brewer(palette = "Set1")` | same | `scale_colour_brewer(palette=Brewer.Set1)` |
| `scale_y_continuous(limits = c(0, 50))` | `limits=(0, 50)` | `scale_y_continuous(limits=(0, 50))` |
| `coord_flip()`, `coord_cartesian(xlim = c(1, 5))` | same | `coord_flip()`, `coord_cartesian(xlim=(1, 5))` |
| `facet_wrap(~class, ncol = 3)` | `facet_wrap("class", ncol=3)` | `facet_wrap("class", ncol=3)` |
| `facet_grid(drv ~ cyl)` | `facet_grid("drv ~ cyl")` | `facet_grid(rows="drv", cols="cyl")` |
| `theme_minimal(base_size = 12)` | same | `theme_minimal(base_size=12)` |
| `theme(legend.position = "bottom")` | `theme(legend_position="bottom")` | `theme(legend_position=Legend.Bottom)` |
| `element_text(face = "bold")`, `element_blank()` | same | `element_text(face=Face.Bold)`, `element_blank()` |
| `labs(title =, x =, colour =)` | same | `labs(title=, x=, colour=)` |
| `ggsave("p.png", width = 6, height = 4, dpi = 300)` | `p.save(...)` | `p.save("p.png", width=6, height=4, dpi=300)?` |
| Message: `` `stat_bin()` using `bins = 30` `` | same | A note in the render result |
| Error: object `x` not found (at print) | at draw | `PlotError.UnknownColumn` from `ggplot(...)?` |

The [examples page](std-plot/examples.md) shows six classic plots side by side
in ggplot2, plotnine and `std.plot`, and a complete program.

## Design

### Data: from Plenty values to a plot table

ggplot2 plots a data frame; anything else first goes through `fortify()`.
`std.plot` keeps that split. Typing happens at the boundary, where Plenty knows
the record type. The grammar then works on a `Table` of named columns, exactly
as ggplot2 works on a data frame.

```text
pub class Table              # immutable after construction; columns behind Shared
    pub def share(self) -> Table          # another owner of the same columns; no allocation
    pub def height(self) -> i64
    pub def names(self) -> list[str] ...  # (Result-returning in the real signature)

enum Column                   # what a table column holds
    Num(list[f64])            # continuous
    Int(list[i64])            # continuous unless wrapped in factor()
    Text(list[str])           # discrete
    Bool(list[bool])          # discrete
    MaybeNum(list[Option[f64]])   # missing values allowed
    MaybeText(list[Option[str]])
```

Ways in, in the order they become available:

1. **Columns.** `table([("wt", Column.Num(wts)), ("cyl", Column.Int(cyls))]?)?`
   moves owned lists in, without copying. `columns(&values)` copies a
   `dict[str, list[f64]]`; the dictionary's key order becomes column order.
   Unequal lengths and duplicate names fail with `PlotError.ShapeMismatch` and
   `PlotError.DuplicateColumn`.
2. **Records with accessors.** `rows(&cars, fields)` evaluates one typed
   accessor per column and copies the results:

   ```text
   cars_table = rows(&cars, [
       num("wt", def(c: &Car) -> f64: c.wt)?,
       num("mpg", def(c: &Car) -> f64: c.mpg)?,
       integer("cyl", def(c: &Car) -> i64: c.cyl)?,
       text("model", def(c: &Car) -> str: c.model)?,
   ]?)?
   ```

   `num`, `integer`, `text` and `flag` box their callback and infer `T` from it
   (probed). The accessor list lives only for the call; the table holds plain
   columns. Derived values go here too, replacing ggplot2's `aes(y = 2 * hp)`:
   `num("kpl", def(c: &Car) -> f64: c.mpg * 0.425)?`.
3. **Records by field name**, after [B77](derived-serialization.md):
   `records(&cars)?` drives each row's derived `__encode__` into a column
   builder that implements `std.serial.Encoder[PlotError]`. Scalar fields
   become columns named after the fields, with no intermediate tree. Nested
   records, lists and enums with payloads are rejected with the field path. A
   `fields=["wt", "mpg"]` argument limits the copy. This is the closest Plenty
   gets to `ggplot(mtcars, ...)` and should become the recommended path.
4. **`std.dataframe`**, through a bridge module that only dataframe users
   import, so `std.plot` never links the Polars engine.
   `ggplot_frame(&frame, mapping, components)` asks the core for the columns
   the specification uses (`required_columns`), checks them against
   `frame.schema()`, and extracts only those with `values_f64`,
   `values_str_nullable` and the other typed readers. Aesthetics may also be
   dataframe `Expr` values, evaluated by one `select` before extraction. That
   is the dataframe counterpart of ggplot2's expressions in `aes()` and of
   plotnine's string expressions. A later version can push `stat_count` and
   `stat_bin` into Polars `group_by`.

Lists and tuples enter through `columns` or `table`. A `list[(f64, f64)]` of
points goes through `rows` with two accessors.

### Aesthetic mappings

```text
pub enum Unmapped:
    Unset

pub type Aes = str | Factor | AfterStat | Stage | Unmapped

pub def aes(x: Aes = Unmapped.Unset, y: Aes = Unmapped.Unset,
            colour: Aes = Unmapped.Unset, color: Aes = Unmapped.Unset,
            fill: Aes = ..., alpha: Aes = ..., size: Aes = ..., shape: Aes = ...,
            linetype: Aes = ..., linewidth: Aes = ..., group: Aes = ...,
            label: Aes = ..., weight: Aes = ..., xmin: Aes = ..., xmax: Aes = ...,
            ymin: Aes = ..., ymax: Aes = ..., xend: Aes = ..., yend: Aes = ...) -> Mapping
```

- A `str` names a column. `aes("wt", "mpg")` works positionally because `x`
  and `y` come first, as in ggplot2.
- `factor("cyl")` treats a numeric column as discrete. Levels are the sorted
  distinct values; `factor("class", levels=[...]?)` orders them explicitly.
- `after_stat("count")` names a variable computed by the layer's stat. Each
  stat documents its variables (`count`, `density`, `ncount`, `ndensity`,
  `prop`, `width`), as in ggplot2.
- `stage(start="x", after_stat="count")` arrives with `after_scale` in a later
  milestone.
- There is no expression language. Computed values belong in the data step,
  where Plenty type-checks them; stat-computed values are limited to the named
  variables above, which ggplot2 provides to cover the common expressions.

Mapping and setting stay distinct, as in ggplot2: `aes(colour="cyl")` maps a
column; `geom_point(colour="red")` sets a constant. ggplot2's classic mistake,
`aes(colour = "blue")`, maps a constant and produces a legend labelled "blue".
In `std.plot` it fails with `UnknownColumn("blue")`, and the message suggests
`geom_point(colour="blue")`.

`colour` and `color` are both accepted, in `aes`, geoms and scales; giving
both is `PlotError.Conflict`. ggplot2 and plotnine accept both spellings, and
users type whichever they learned.

### Checking and errors

Column names in strings cannot be checked at compile time. They are checked
eagerly instead: `ggplot(...)` and `plus(...)` validate every mapping against
the table, every required aesthetic (`geom_point` needs `x` and `y`), and every
setting (colour names, palette sizes), before anything is rendered. The error
arises at the statement that introduced the mistake. ggplot2 and plotnine
report the same mistakes only when the plot is printed or drawn.

```text
pub enum PlotError:
    UnknownColumn(ColumnFault)    # name, aesthetic, layer, available names, nearest match
    MissingAesthetic(AesFault)    # "geom_point requires y"
    WrongKind(AesFault)           # discrete column for a continuous-only scale, etc.
    ShapeMismatch(ShapeFault)
    DuplicateColumn(str)
    Conflict(str)                 # colour and color both given; two coords
    InvalidSetting(SettingFault)  # unknown colour name, negative size, bins = 0
    Font(FontError)
    Io(IoError)
    Allocation(AllocError)
```

Faults name the layer by position and geom (`layer 2 (geom_smooth)`), the
aesthetic, and the column, and print through
[`std.report`](../design/stdlib/report.md) like other std errors:

```text
error: layer 1 (geom_point): aesthetic `colour` maps unknown column "cly"
  available columns: wt, mpg, cyl, gear, model
  did you mean "cyl"?
```

Statically typed handles would move these checks to compile time. Plenty's
`clist` already has a compile-time selector, `Car.wt`, accepted only by
`column`. Generalising it into a typed `Field[Car, f64]` value would allow
`aes(x=Car.wt, colour=factor(Car.cyl))`, checked by the compiler. That is a
language change; [decision 6](#decisions) defers it
after `records` ships, rather than designing it now.

### Composition

Plenty has operator hooks and explicit consuming method receivers. For this API,
`+` would be a poor fit: every step can allocate, and
Plenty makes that visible, so a chain would read
`((ggplot(t, m)? + geom_point())? + labs(title="x"))?`, as `str` already
requires `("a" + b)?`.

`std.plot` therefore composes in two ways that exist today:

```text
p = ggplot(cars, aes("wt", "mpg", colour=factor("cyl")), [
    geom_point(size=2),
    geom_smooth(method=Smooth.Lm),
    labs(title="Fuel economy", colour="Cylinders"),
    theme_minimal(),
]?)?

with_line = p.plus([geom_line()]?)?        # p is unchanged
```

- `ggplot(data, mapping, components)` takes the components as a list, in
  order, one per line. It reads top to bottom like a `+` chain. Later
  components override earlier ones by the same rules as ggplot2: a second
  `x` scale replaces the first, a complete theme replaces the theme, and
  `theme(...)` modifies it.
- `p.plus(components)` borrows `p` and returns a new plot that shares `p`'s
  table and copies its component list (probed, including chaining on
  temporaries: `base.plus(a)?.plus(b)?`). This is ggplot2's
  `base <- ggplot(...); base + geom_point(); base + geom_line()` idiom, and
  the way to add components conditionally: `if smooth: p = p.plus(...)?`.

Every component constructor returns one enum, `Component`, so a single list
holds layers, scales, coords, facets, labels and themes. This mirrors adding a
`list()` of components in ggplot2. There is no `p.add(...)` mutator: a plot is
immutable once built, so validation happens once, and sharing a plot or its
table between threads needs no locks.

### Ownership and allocation

- `ggplot` takes its `Table` by value. A table holds its columns in a
  `Shared` owner; `table.share()` gives another plot the same columns without
  copying. Tables are immutable, so sharing has value semantics.
- Components are plain data: enums, numbers, strings and lists. They hold no
  closures, so a component list can be copied with `copy` and a plot can be
  rebuilt or shared freely.
- Allocation happens in four visible places, each fallible: building the table
  (copies columns, except `table` which moves them), building the plot (the
  component list), rendering (per-layer computed data, scale breaks, text, and
  for PNG a pixel buffer of `width × height × 4` bytes), and writing.
  Rendering to an SVG stream writes directly to a `std.io` writer, so the
  document is never held in memory. Nothing allocates behind a non-`Result`
  call.
- Rendering borrows the plot; one plot can be saved in several formats.

### The build pipeline

Rendering follows ggplot2's `ggplot_build()` then `ggplot_gtable()`:

1. **Resolve mappings** per layer: inherit the plot mapping unless the layer
   sets `inherit_aes=False`; read columns.
2. **Facet**: assign each row to a panel.
3. **Transform scales** (`log10`, `sqrt`, `reverse`) before statistics, as
   ggplot2 does.
4. **Group** by the interaction of discrete mappings and `group`.
5. **Compute stats** per panel and group (`identity`, `count`, `bin`,
   `smooth`, `boxplot`, `density`); resolve `after_stat` mappings.
6. **Adjust positions**: `identity`, `stack`, `fill`, `dodge`, `jitter`.
7. **Train and map scales**: ranges and levels, breaks, labels, palettes;
   merge legends that share a title and levels.
8. **Coordinates**: Cartesian limits (zoom without dropping data), flip,
   fixed aspect ratio; polar later.
9. **Lay out**: measure text, size axes, strips, legends and titles, place
   panels.
10. **Draw** primitives to a canvas: rectangles, polylines, polygons, circles,
    text with anchor and rotation, clip regions, groups.

Each stage is a Plenty function over columns of `f64` and level indices.
Stages 1–9 are independent of the output format.

### Layers, stats and positions

| Milestone | Geoms (default stat) | Stats | Positions |
| --- | --- | --- | --- |
| 1 | `point`, `line`, `path`, `col`, `bar` (count), `histogram` (bin), `smooth` (smooth, `Lm` only), `hline`, `vline`, `abline`, `segment`, `text` | `identity`, `count`, `bin`, `smooth` | `identity`, `stack`, `fill`, `dodge` |
| 2 | `area`, `ribbon`, `boxplot`, `violin`, `density`, `freqpoly`, `errorbar`, `pointrange`, `linerange`, `tile`, `raster`, `rect`, `label`, `step`, `jitter` | `boxplot`, `density`, `ydensity`, `summary`, `smooth` with `Loess` | `jitter` (seeded), `dodge2` |
| 3 | `contour`, `density_2d`, `bin_2d`, `hex`, `polygon`, `quantile` | `contour`, `bin_2d`, `ecdf`, `qq` | `nudge` |

`geom_smooth` draws ggplot2's 95% confidence ribbon (`se=True`). Linear
regression standard errors need the Student t quantile, implemented in Plenty
from the regularised incomplete beta function. Loess follows R's
`stats::loess` defaults (span 0.75, degree 2, tricube weights). ggplot2 picks
loess below 1,000 rows and `mgcv` GAMs above; GAMs are not planned (see
[decision 5](#decisions)). [`std.stat`](../design/stdlib/stat.md) already provides means, variances,
quantiles and normal, uniform and exponential distributions (`Normal` has
`cdf` and `inverse_cdf`); `std.plot` should use it and add the Student t
distribution there rather than in the plot module.

Jitter uses a seeded `std.random.Rng` (default seed fixed, settable with
`position_jitter(seed=...)`), so output stays deterministic.

### Scales and guides

| Milestone | Scales |
| --- | --- |
| 1 | `scale_{x,y}_continuous`, `_discrete`, `_log10`, `_sqrt`, `_reverse`; `scale_{colour,fill}_hue` (default discrete), `_gradient` (default continuous), `_gradient2`, `_viridis_d`, `_viridis_c`, `_manual`, `_grey`; `scale_size`, `scale_alpha`, `scale_shape`; `xlim`, `ylim`, `lims` |
| 2 | `_brewer`, `_distiller`, `_gradientn`, `_identity`, `scale_linetype`, date and datetime scales over `std.time` and dataframe dates, `guides()`, `guide_legend`, `guide_colourbar` options |
| 3 | Binned scales, `sec_axis`, custom transforms |

Arguments follow ggplot2: `name`, `breaks`, `labels`, `limits`, `expand`,
`na_value`, `palette`. Breaks use Wilkinson's extended algorithm (as R's
`scales::extended_breaks` does). Labels for continuous scales format numbers
with the same rules as `scales::label_number` defaults; `labels=` accepts a
list of strings. A function-valued `labels=` would put a closure in the plot;
instead a small `LabelFormat` enum covers the common cases (`Percent`,
`Comma`, `Dollar`, `Scientific`, `Number(accuracy)`).

Colours are names (R's 657 colour names, `grey0`–`grey100`) or hex
`"#RRGGBB"` and `"#RRGGBBAA"`. The default discrete palette is ggplot2's HCL
hue palette (hue 15–375, chroma 100, luminance 65); continuous is the
`#132B43`–`#56B1F7` gradient. ColorBrewer and viridis tables ship as generated
Plenty data. Legends are generated from every non-position scale; scales with
the same title and levels merge into one legend, as in ggplot2.

### Coordinates and facets

- Milestone 1: `coord_cartesian(xlim=, ylim=, expand=)`, `coord_flip()`,
  `coord_fixed(ratio=)`; `facet_wrap(facets, nrow=, ncol=, scales=)` and
  `facet_grid(rows=, cols=, scales=, space=)` with `Scales.Fixed`, `Free`,
  `FreeX`, `FreeY`. Facet variables are column names or lists of names.
- Milestone 2: `labeller` choices (`LabelBoth`, `LabelValue`), `margins`.
- Milestone 3: `coord_polar`, `coord_radial`, `coord_trans`.

Formula strings (`"drv ~ cyl"`) are not accepted; `rows=` and `cols=` are the
single spelling, matching ggplot2's `vars()` interface. A string containing
`~` fails with a hint.

### Themes and labels

The default is `theme_grey` (grey panel, white grid), the look ggplot2 users
recognise. `theme_bw`, `theme_minimal`, `theme_classic`, `theme_light`,
`theme_dark`, `theme_void` and `theme_linedraw` take `base_size`,
`base_family`, `ink` and `paper`.

`theme(...)` takes ggplot2's element names with dots replaced by underscores,
as plotnine does: `legend_position`, `axis_text_x`, `panel_grid_minor`,
`plot_title`. Each parameter defaults to "inherit", so a theme call changes
only what it names. Element inheritance (`axis_text_x` inherits `axis_text`,
which inherits `text`) follows ggplot2's element tree. ggplot2 has about 130
theme elements; the full set fits within the 255-parameter limit, and
milestone 1 ships the ~40 most used. Element constructors are
`element_text(size=, face=, colour=, angle=, hjust=, vjust=, margin=)`,
`element_line`, `element_rect` and `element_blank()`. Enumerated settings are
enums (`Legend.Bottom`, `Face.Bold`) rather than strings, so typos fail to
compile.

`labs(title=, subtitle=, caption=, tag=, x=, y=, colour=, fill=, ...)`,
`xlab()`, `ylab()` and `ggtitle()` set labels. Without them, axis and legend
titles are the mapped column names, as in ggplot2.

### Notes instead of messages

ggplot2 prints messages and warnings: the bin count chosen by `stat_bin`, the
smoothing method chosen by `geom_smooth`, rows removed for missing values.
Plenty has no warning channel, and a library must not write to standard error
on its own. Rendering returns a `Rendered` value whose `notes` list carries the
same information; `print(rendered.notes)?` shows it. `save` is the convenience
that writes the file and returns the notes. `na_rm=True` on a layer silences
the missing-value note, as in ggplot2.

## Rendering and output

### Formats

1. **SVG first** (milestone 1). It is text, so output is easy to test, diff
   and view, and needs no rasteriser. `p.save("out.svg", ...)` or
   `p.write_svg(&mut writer, ...)` for any `std.io` writer.
2. **PNG** (milestone 2): an anti-aliased scanline rasteriser in Plenty
   (signed-area coverage accumulation, the technique used by `font-rs` and
   `stb_truetype`), then PNG chunks assembled in Plenty with
   `std.compress.compress(Format.Zlib, ...)` for `IDAT` and
   `std.compress.crc32` for chunk checksums.
3. **PDF** later, if requested; it shares the SVG canvas model.

`save` chooses the format from the extension, with `width`, `height`, `units`
(`Units.In`, `Cm`, `Mm`, `Px`; inches by default) and `dpi` (default 300), as
`ggsave` does.

### Text and fonts

Layout needs text measurement even for SVG: axis label widths decide panel
size, and legend widths decide plot width. ggplot2 asks the graphics device.
`std.plot` uses bundled font data:

- **Metrics** (advance widths and kerning pairs) for a default sans-serif font
  are generated into the module, so measurement needs no file system access and
  gives the same layout on every machine. The recommended font is Liberation
  Sans (SIL Open Font License), which is metric-compatible with Arial and
  Helvetica. An SVG names `font-family="Liberation Sans, Arial, Helvetica,
  sans-serif"`, so viewers without the bundled font substitute one with the
  same widths and the measured layout still holds.
- **Outlines** for PNG and for SVG with `text=Text.Paths` (exact rendering
  without viewer fonts) come from a subset of the same font, embedded as a
  `b"..."` byte literal and parsed in Plenty (`cmap`, `hmtx`, `kern`, `glyf`
  quadratic outlines). The subset covers Latin-1, Latin Extended-A, Greek, and
  plotting symbols (minus sign, multiplication sign, degree, micro, en dash).
  Emission removes it from programs that never draw outlines.
- `font_file(path)` loads another TrueType font at run time; `base_family`
  then names it.
- Missing glyphs render as the font's `.notdef` and add a note.
- Shaping is left-to-right with kerning only. Complex scripts are out of scope;
  if they are needed later, shaping is the one piece where wrapping an
  established engine is justified, subject to the allocation rule below.

### Deterministic output

Given the same plot and toolchain, output bytes are identical:

- coordinates are written with fixed precision (two decimals in SVG user
  units), elements in layer order, clip and gradient identifiers numbered
  sequentially;
- no timestamps, random identifiers or host font lookup;
- jitter uses a fixed default seed;
- the rasteriser is Plenty floating-point code with no target-dependent fast
  paths, and PNG compression uses a fixed level of the pinned `zlib-rs`.

Tests compare SVG snapshots, like ggplot2's `vdiffr` (which also renders
through an SVG device with bundled Liberation fonts), and compare decoded PNG
pixels rather than compressed bytes.

### Write in Plenty or wrap

| Part | Choice | Reason |
| --- | --- | --- |
| Grammar: mappings, stats, scales, guides, facets, layout | Plenty | No crate implements it; plotters does not. This is most of the work in any case. |
| Statistics (bins, `lm`, loess, density, quantiles) | Plenty | Formulas over lists; builds on `std.stat` |
| Colour (HCL conversion, palettes) | Plenty | Formulas and tables |
| SVG writer | Plenty | Text generation, a few hundred lines |
| Font parsing and text measurement | Plenty | Table parsing over bytes; `ab_glyph` and `font-kit` allocate infallibly, and `font-kit` adds host font discovery |
| Rasteriser | Plenty | Short, numeric code the compiler should handle well; `tiny-skia` allocates infallibly. Measure before milestone 2 closes |
| zlib compression and CRC-32 | Existing native `std.compress` (`zlib-rs`) | Already approved, reports allocation failure |
| Complex-script shaping | Not planned; the only candidate to wrap | A large established engine |

**plotters is not recommended as a backend.** It would replace only the SVG
writer and rasteriser, the smallest parts of the work. Its layout model (label
areas sized in pixels by the caller) does not match ggplot2's, which sizes
from measured text, so `std.plot` would compute layout anyway and pass
primitives across a foreign boundary. It allocates through Rust's global
allocator, which aborts on failure; the
[native-wrapper rule](standard-library-scope.md#plenty-or-native) rejects that
for new wrappers. Its bitmap backend brings `image` and `font-kit`, with
system font lookup that breaks deterministic output. Its role here is a
reference for primitive drawing and a comparison for rasteriser tests.

If the Plenty rasteriser cannot reach acceptable speed (target: a 2,400 ×
1,800 PNG with 100,000 points in under 0.5 s on the reference laptop), the
response is codegen work or `std.simd`, per the
[Plenty-first preference](standard-library-scope.md#plenty-or-native), before
any wrapper.

## Costs

- **Source size.** Milestone 1 is likely 10–15k lines of Plenty, comparable
  to `std.markdown` plus `std.regex`. Importing a large std module costs
  front-end time; measure the compile time of a one-plot program at each
  milestone and keep it within a budget agreed before milestone 1 (proposed:
  under 150 ms over hello world with a warm cache).
- **Executable size.** Emission drops unused std functions, so a program that
  only writes SVG carries neither the rasteriser nor the font outlines. Add a
  size check like the existing executable-size budgets.
- **Runtime.** Each stage is linear in rows except sorting for lines,
  quantiles and loess (O(n²) per fit before interpolation). Rendering a
  million-point scatter to SVG produces a file of tens of megabytes, as
  in ggplot2; PNG is the format for large data.

## Phased plan

1. **Core and SVG.** `Table`, `table`, `columns`, `rows`; `aes`, `factor`,
   `after_stat`; milestone-1 geoms, stats, positions, scales, coords, facets
   and themes; layout with bundled metrics; SVG output; `PlotError`; notes;
   reference page, tutorial lesson, SVG snapshot tests. Exit: the six
   [examples](std-plot/examples.md) render and read correctly next to
   ggplot2's output.
2. **Breadth and PNG.** Milestone-2 geoms and scales, loess, jitter, dates,
   glyph outlines, rasteriser and PNG, `text=Text.Paths`.
3. **Records by name.** `records()` over B77's `Encoder` once B77 lands.
4. **Dataframes.** The bridge module with `Expr` aesthetics and required-column
   extraction; later, stat push-down into Polars.
5. **Milestone-3 geoms and coords**, as demand shows.

Stages 3 and 4 depend on other work and can move earlier if it lands first.

## Decisions

The review accepted these choices. Each records the alternative considered.

1. **ggplot2 names verbatim**, not Plenty-style names such as `plot.point()`.
   Recognition is the goal, and plotnine shows the names work outside R. Users
   import with `from std.plot import ggplot, aes, geom_point` or
   `import std.plot as gg`; wildcard imports are not proposed.
2. **Both `colour` and `color` are accepted**; a call that gives both is
   `PlotError.Conflict`. This is two spellings for one thing, against the
   preference for single rules, but ggplot2 users write both and the cost is
   one parameter per colour aesthetic.
3. **Composition uses the component list and `plus`.** No operator hooks are
   added for this: the `?` after every step would make `+` worse than the list.
4. **ggplot2's implicit defaults stay** (`bins=30`, the smoothing method), each
   reported in the notes as ggplot2's messages do. The plot shows the effect
   immediately, and requiring them would break the "write it as in ggplot2"
   goal.
5. **`geom_smooth` uses loess at every size**, where ggplot2 switches to an
   `mgcv` GAM above 1,000 rows. Large inputs are evaluated on an interpolation
   grid as R's `loess(surface = "interpolate")` does, and the note says so.
   Until loess lands in milestone 2, the default is `Smooth.Lm`, also noted.
6. **No typed column handles (`aes(x=Car.wt)`) for now.** Strings checked at
   construction match ggplot2's behaviour. Revisit after B77 lands: the
   per-column record accessor (`num("wt", def(c: &Car) -> f64: c.wt)?`) is
   verbose, and if `records()` does not remove most of that boilerplate, or
   mistyped column names prove common, typed handles become worth a language
   change.
7. **Default font: Liberation Sans**, for Arial/Helvetica metric
   compatibility and the OFL licence. DejaVu Sans has wider coverage but
   different metrics from common viewer fonts.
8. **PNG uses a Plenty rasteriser**, not a `tiny-skia` wrapper, with the speed
   target in [Write in Plenty or wrap](#write-in-plenty-or-wrap) as the
   acceptance test.
9. **Missing values**: rows missing any aesthetic a layer uses are dropped and
   counted in the notes, as ggplot2 does; `na_rm=True` silences the note.
10. **`std.plot` belongs in `std`**, beside `std.dataframe`, not in a
    first-party package. The [scope](standard-library-scope.md) aims at data
    tools without third-party packages, and emission removes what a program
    does not use.
11. **The dataframe bridge is a separate submodule** under `std.plot`'s
    namespace (as `std.http` has submodules), imported only by dataframe users,
    so neither module forces the other's dependencies.

## Risks

- **Scope.** ggplot2 has about 50 geoms, 30 stats and over 100 scales. The
  milestones cut this to the plots most people make; the risk is users
  arriving with the missing tenth.
- **Fidelity expectations.** Users will compare against ggplot2 output.
  Differences in fonts, break placement or smoothing will be reported as bugs.
  Matching defaults and testing against R reference values (breaks, `lm`
  standard errors, loess fits, histogram bins) limits this.
- **Text.** Without shaping, labels in complex scripts render incorrectly.
  SVG viewers without metric-compatible fonts can misplace text slightly;
  `text=Text.Paths` is the exact fallback.
- **Runtime checking of names.** Mapping errors appear when the program runs,
  not when it compiles. Eager validation at construction and suggestions
  limit the cost.
- **Compile time and binary size** of a large std module; the budgets above
  make regressions visible.
- **Numerical accuracy** of statistics written from scratch. Test against R.
- **Dependencies on unimplemented work**: `records()` waits for B77; the
  dataframe bridge depends on the dataframe module's typed readers staying
  stable.

## References

- Hadley Wickham, "A Layered Grammar of Graphics", *Journal of Computational
  and Graphical Statistics* 19(1), 2010.
- [ggplot2 reference index](https://ggplot2.tidyverse.org/reference/index.html)
  and [changelog](https://ggplot2.tidyverse.org/news/index.html) (4.0.3).
- [plotnine reference](https://plotnine.org/reference/), in particular
  [`aes`](https://plotnine.org/reference/aes.html) and
  [`ggplot`](https://plotnine.org/reference/ggplot.html) (0.15.8).
- [plotters documentation](https://docs.rs/plotters/latest/plotters/) (0.3.7).
- [Derived serialization](derived-serialization.md),
  [Polars dataframes](polars-dataframes.md),
  [standard library scope](standard-library-scope.md).

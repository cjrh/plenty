# Plot uncertainty and intervals

[`std.plot`](plot.md) draws caller-supplied lower and upper bounds:

- `geom_errorbar` draws a vertical interval with end caps.
- `geom_linerange` draws the interval without caps.
- `geom_ribbon` fills the area between bounds, sorted by x within each group.

Map bounds with `aes(x="time", ymin="lower", ymax="upper")`. A y mapping is
optional and ignored by these geoms, so they can inherit a centre-value mapping
shared with points or a line. All three accept layer data/mapping overrides,
colour, alpha, linewidth and `na_rm`. Ribbons also accept fill.
They do not compute statistical estimates, errors or confidence levels.

## Bounds and transformations

Both bounds must be continuous columns. Finite `ymin > ymax` is an error at
construction. Equal bounds are valid. Both endpoints contribute to axis
training, including in facets.

Position transforms and limits apply to each bound. A row with a missing,
non-finite, out-of-domain or scale-excluded endpoint is omitted, with a note
unless `na_rm=True`. Use coordinate limits to clip a visible interval without
removing its data. Reversed axes and `coord_flip` preserve the interval.

Linewidth is in millimetres. Error-bar `width` is the total cap width in
transformed x units (category spacing for discrete x); it defaults to 0.2
and currently must be in (0, 1]. On a log x axis it therefore describes a
distance in log space. To display horizontal intervals, use `coord_flip`.
Independent `xmin`/`xmax` mappings are not implemented.

## Ribbon grouping and gaps

Ribbons require continuous x. Discrete colour/fill mappings or an explicit
group define separate bands. Colour and fill must be constant within each
continuous segment of a group; differing styles produce a rendering error
instead of arbitrarily choosing one. Duplicate x values within a segment
also produce an error. Use distinct x values for a well-defined band.

Missing bounds at a finite x split a ribbon into separate polygons. A segment
needs at least two valid rows to form a polygon. Coordinates are joined by
straight edges; no smoothing, interpolation between groups, or statistical
fitting occurs. Ribbons use identity positioning, not stacking.

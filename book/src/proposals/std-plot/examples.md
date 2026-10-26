# `std.plot` examples beside ggplot2 and plotnine

**Status: Not implemented.** These examples illustrate the
[`std.plot` proposal](../std-plot.md). The Plenty fences show proposed API and
are not compiled. Each example assumes a table with the columns of the R dataset
named (`mtcars`, `diamonds`, `mpg`) and these imports:

```text
from std.plot import ggplot, aes, factor, after_stat, labs, theme, theme_minimal
from std.plot import geom_point, geom_smooth, geom_histogram, geom_bar
from std.plot import facet_wrap, facet_grid, position_dodge, scale_colour_brewer
from std.plot import Smooth, Brewer, Legend
```

## Scatter plot with a colour mapping

```r
ggplot(mtcars, aes(wt, mpg, colour = factor(cyl))) +
  geom_point(size = 2)
```

```python
(ggplot(mtcars, aes("wt", "mpg", color="factor(cyl)"))
 + geom_point(size=2))
```

```text
p = ggplot(mtcars, aes("wt", "mpg", colour=factor("cyl")), [
    geom_point(size=2),
]?)?
p.save("cars.svg", width=6, height=4)?
```

`factor("cyl")` makes the integer column discrete, so the legend shows three
levels instead of a gradient. `size=2` is accepted because numeric settings take
`i64` as well as `f64`.

## Points with a fitted line

```r
ggplot(mtcars, aes(wt, mpg)) +
  geom_point() +
  geom_smooth(method = "lm")
```

```python
(ggplot(mtcars, aes("wt", "mpg"))
 + geom_point()
 + geom_smooth(method="lm"))
```

```text
p = ggplot(mtcars, aes("wt", "mpg"), [
    geom_point(),
    geom_smooth(method=Smooth.Lm),
]?)?
```

The line carries a 95% confidence band by default (`se=True`), as in ggplot2.

## Histogram

```r
ggplot(diamonds, aes(price)) +
  geom_histogram(binwidth = 500)
```

```python
ggplot(diamonds, aes("price")) + geom_histogram(binwidth=500)
```

```text
p = ggplot(diamonds, aes("price"), [
    geom_histogram(binwidth=500),
]?)?
```

Without `binwidth` or `bins`, the stat uses 30 bins and records a note, the
counterpart of ggplot2's `` `stat_bin()` using `bins = 30` `` message. Density
instead of counts: `aes("price", after_stat("density"))`.

## Bars with fill and dodge

```r
ggplot(diamonds, aes(cut, fill = clarity)) +
  geom_bar(position = "dodge")
```

```python
(ggplot(diamonds, aes("cut", fill="clarity"))
 + geom_bar(position="dodge"))
```

```text
p = ggplot(diamonds, aes("cut", fill="clarity"), [
    geom_bar(position=position_dodge()),
]?)?
```

`geom_bar` counts rows per `cut` and `clarity`, so `y` is not mapped.
`position_dodge(width=0.8)` takes the same arguments as in ggplot2.

## Facets

```r
ggplot(mpg, aes(displ, hwy)) +
  geom_point() +
  facet_wrap(~class, ncol = 3)

ggplot(mpg, aes(displ, hwy)) +
  geom_point() +
  facet_grid(drv ~ cyl)
```

```python
ggplot(mpg, aes("displ", "hwy")) + geom_point() + facet_wrap("class", ncol=3)

ggplot(mpg, aes("displ", "hwy")) + geom_point() + facet_grid("drv ~ cyl")
```

```text
base = ggplot(mpg, aes("displ", "hwy"), [geom_point(),]?)?
wrapped = base.plus([facet_wrap("class", ncol=3)]?)?
grid = base.plus([facet_grid(rows="drv", cols="cyl")]?)?
```

`base` stays usable after each `plus`; both plots share its table.

## Labels, scales and a theme

```r
ggplot(mtcars, aes(wt, mpg, colour = factor(gear))) +
  geom_point(size = 3) +
  scale_colour_brewer(palette = "Set1") +
  labs(title = "Fuel economy by weight",
       x = "Weight (1000 lb)", y = "Miles per gallon", colour = "Gears") +
  theme_minimal(base_size = 12) +
  theme(legend.position = "bottom")
```

```python
(ggplot(mtcars, aes("wt", "mpg", color="factor(gear)"))
 + geom_point(size=3)
 + scale_color_brewer(type="qual", palette="Set1")
 + labs(title="Fuel economy by weight",
        x="Weight (1000 lb)", y="Miles per gallon", color="Gears")
 + theme_minimal(base_size=12)
 + theme(legend_position="bottom"))
```

```text
p = ggplot(mtcars, aes("wt", "mpg", colour=factor("gear")), [
    geom_point(size=3),
    scale_colour_brewer(palette=Brewer.Set1),
    labs(title="Fuel economy by weight",
         x="Weight (1000 lb)", y="Miles per gallon", colour="Gears"),
    theme_minimal(base_size=12),
    theme(legend_position=Legend.Bottom),
]?)?
```

`Brewer.Set1` and `Legend.Bottom` are enum values, so a misspelt palette or
position fails to compile.

## A complete program from records

```text
from std.plot import ggplot, aes, factor, geom_point, geom_smooth, labs
from std.plot import rows, num, integer, text, Smooth, PlotError

class Car:
    model: str
    wt: f64
    mpg: f64
    cyl: i64

def main() -> Result[(), PlotError | AllocError | IoError]:
    cars = [
        Car("Mazda RX4", 2.620, 21.0, 6),
        Car("Datsun 710", 2.320, 22.8, 4),
        Car("Hornet Sportabout", 3.440, 18.7, 8),
    ]?

    table = rows(&cars, [
        text("model", def(c: &Car) -> str: c.model)?,
        num("wt", def(c: &Car) -> f64: c.wt)?,
        num("mpg", def(c: &Car) -> f64: c.mpg)?,
        integer("cyl", def(c: &Car) -> i64: c.cyl)?,
    ]?)?

    p = ggplot(table, aes("wt", "mpg", colour=factor("cyl")), [
        geom_point(size=2),
        geom_smooth(method=Smooth.Lm, colour="grey30"),
        labs(x="Weight (1000 lb)", y="Miles per gallon", colour="Cylinders"),
    ]?)?

    notes = p.save("cars.svg", width=6, height=4)?
    print(notes)?
    Ok(())
```

`geom_smooth(colour="grey30")` is a setting: one fixed colour, so the line
does not split by the plot's `colour` mapping. In ggplot2 the same setting also
overrides the inherited mapping for that layer.

After [derived serialization](../derived-serialization.md) lands, the `rows`
call can become `records(&cars)?` for a class marked `@serial`.

## A mistake and its error

```text
p = ggplot(table, aes("wt", "mpg", colour="cly"), [geom_point(),]?)?
```

```text
error: plot mapping: aesthetic `colour` maps unknown column "cly"
  available columns: model, wt, mpg, cyl
  did you mean "cyl"?
```

The error comes from `ggplot(...)`, before rendering. ggplot2 reports
``object 'cly' not found`` when the plot is printed.

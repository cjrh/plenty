# Join and rank dataframe results

An order table and a price table usually have different shapes. Join them by
product, derive revenue, then group and rank the results. These operations stay
inside the optional Polars engine until `collect()`.

Methods borrow their inputs automatically, including temporary expressions.
The example uses named intermediate plans to keep the steps readable; each
step still belongs to one lazy query.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    orders = df.frame([df.column_i64("product", [1, 2, 1, 3]?)?, df.column_i64("units", [2, 1, 3, 5]?)?]?)?
    prices = df.frame([df.column_i64("product", [1, 2]?)?, df.column_f64("price", [10.0, 25.0]?)?]?)?
    keys = [df.col("product")?]?
    joined = orders.lazy()?.join(prices.lazy()?, keys, keys, how=df.JoinKind.Left)?
    revenue = df.col("price")?.fill_null(df.lit(0.0)?)?.mul(df.col("units")?)?.alias("revenue")?
    enriched = joined.with_columns([revenue]?)?
    totals = enriched.group_by(keys, [df.col("revenue")?.sum()?.alias("total")?]?)?
    ranking = [df.SortKey(df.col("total")?, descending=True), df.SortKey(df.col("product")?)]?
    best = totals.sort(ranking)?.limit(2)?.collect()?
    print(best.values_i64("product")?)?
    print(best.values_f64("total")?)?
    Ok(())
```
```output
[1, 2]
[50.0, 25.0]
```

The left join keeps product 3 even though it has no price. This example
deliberately treats a missing price as zero with `fill_null`; a real
application may instead reject or separately report unpriced orders.
The default inner join would drop unmatched orders.

Duplicate keys produce every matching pair. The price table here has one
row per product; duplicate price rows would multiply revenue contributions.
The join does not validate uniqueness. Null keys do not match unless you
pass `nulls_equal=True`.

Both key columns survive the join. Since both are named `product`, the
right one becomes `product_right`; use `suffix="_price"` to choose another
suffix. A suffix collision is an error, never silent replacement.

The ranking sorts totals descending, then product ascending to break ties.
Nulls go last by default; each `SortKey` can specify `nulls_last=False`.
Sorting is stable for equal keys, but a join or group has no promised input
order, so use a meaningful tie-breaking key when you need reproducible output.

For pagination, replace `limit(2)` with `slice(2, 2)` to skip two rows and
take up to two more. Dataframe slicing takes an offset and **count**, not
start/stop endpoints. Negative offsets count from the end; invalid negative
lengths return an error. See the [query contracts](../../design/stdlib/dataframe.md#joins-ordering-and-row-windows).

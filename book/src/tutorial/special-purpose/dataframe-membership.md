# Membership and full joins

Use a semi join to keep rows whose keys appear in another dataset, or an
anti join to find keys that are absent. Both return only the left columns.
They are useful for checking allowed IDs, finding missing records and
filtering against a lookup table without multiplying rows.

These examples require the [optional engine](../../design/stdlib/dataframe-engine.md).

## Keep matching rows or find missing ones

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    orders = df.frame([
        df.column_i64("id", [1, 2, 2, 3]?)?,
        df.column_str("order", ["one", "two-a", "two-b", "three"]?)?
    ]?)?
    allowed = df.frame([df.column_i64("id", [2, 2]?)?]?)?
    keys = [df.col("id")?]?
    order = [df.SortKey(df.col("order")?)]?
    matching = orders.lazy()?.join(
        allowed.lazy()?, keys, keys, how=df.JoinKind.Semi
    )?.sort(order)?.collect()?
    missing = orders.lazy()?.join(
        allowed.lazy()?, keys, keys, how=df.JoinKind.Anti
    )?.sort(order)?.collect()?
    print(matching.values_str("order")?)?
    print(missing.values_str("order")?)?
    Ok(())
```
```output
["two-a", "two-b"]
["one", "three"]
```

The duplicate allowed ID does not duplicate matching orders. Each left row
is kept at most once; duplicate left rows would still be separate rows.
No right-side columns or suffixes appear in the result.

Null keys do not match by default, so a null left key goes into the anti
result. Set `nulls_equal=True` only when null should be an ordinary matching
key. Sorting above makes presentation deterministic.

## Reconcile both sides with a full join

A full join also keeps rows found only on the right. Both key columns remain,
so explicitly derive a common display key if that is what your report needs.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    before = df.frame([
        df.column_i64("id", [1, 2]?)?,
        df.column_i64("price", [10, 20]?)?
    ]?)?
    after = df.frame([
        df.column_i64("id", [2, 3]?)?,
        df.column_i64("price", [25, 30]?)?
    ]?)?
    keys = [df.col("id")?]?
    joined = before.lazy()?.join(after.lazy()?, keys, keys, how=df.JoinKind.Full)?
    display_id = df.col("id")?.fill_null(df.col("id_right")?)?.alias("item")?
    result = joined.with_columns([display_id]?)?.sort([
        df.SortKey(df.col("item")?)
    ]?)?.collect()?
    print(result.values_i64("item")?)?
    print(result.values_i64_nullable("price")?)?
    print(result.values_i64_nullable("price_right")?)?
    Ok(())
```
```output
[1, 2, 3]
[Option[i64].Some(10), Option[i64].Some(20), Option[i64].Nothing]
[Option[i64].Nothing, Option[i64].Some(25), Option[i64].Some(30)]
```

Full joins preserve duplicate matching pairs, just like inner and left joins.
They do not verify key uniqueness. The nulls above describe missing sides;
they are not query failures. See the [join contracts](../../design/stdlib/dataframe-joins.md)
for composite keys, null matching, name collisions and ordering.

Continue with [Match nearby observations](dataframe-asof.md) when a lookup
should use the most recent or nearest key rather than an identical key.

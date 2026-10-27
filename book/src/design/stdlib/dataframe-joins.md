# Dataframe equality joins

`Query.join(other, left_keys, right_keys, how=JoinKind.Inner,
suffix="_right", nulls_equal=False)` returns a new immutable query.
It borrows both plans and expression lists; the resulting query remains
valid after those owners are dropped.

Key lists must be nonempty and equally long. Multiple expressions form
composite keys, matching every component. Key dtypes must be compatible;
there is no implicit Plenty-side cast. Empty/unequal lists fail immediately
with `DataFrameError.Schema`; missing columns, incompatible dtypes and
other native plan errors can fail during schema resolution or collection.

| Kind | Rows kept | Output columns |
|---|---|---|
| `Inner` | Every matching left/right pair | Both sides |
| `Left` | Matching pairs, plus each unmatched left row | Both sides; unmatched right cells are null |
| `Full` | Matching pairs, plus each unmatched row from either side | Both sides; absent-side cells are null |
| `Semi` | Each left row with at least one right match | Left side only |
| `Anti` | Each left row with no right match | Left side only |

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    orders = df.frame([df.column_i64("product", [3, 1, 2, 1]?)?, df.column_i64("units", [5, 2, 1, 3]?)?]?)?
    prices = df.frame([df.column_i64("product", [1, 2]?)?, df.column_f64("price", [10.0, 25.0]?)?]?)?
    keys = [df.col("product")?]?
    order = [df.SortKey(df.col("product")?), df.SortKey(df.col("units")?)]?
    priced = orders.lazy()?.join(prices.lazy()?, keys, keys)?.sort(order)?.collect()?
    print(priced.values_i64("product")?, priced.values_f64("price")?)?
    unpriced = orders.lazy()?.join(prices.lazy()?, keys, keys, how=df.JoinKind.Anti)?.collect()?
    print(unpriced.values_i64("product")?)?
    Ok(())
```
```output
[1, 1, 2] [10.0, 10.0, 25.0]
[3]
```

## Multiplicity and validation

Inner, left and full joins are many-to-many. Two equal left keys and three
equal right keys yield six matching pairs. No uniqueness check is performed;
these are not validated one-to-one or many-to-one lookups.

Semi and anti joins test membership. Duplicate right matches do not multiply
the output. Duplicate left rows remain distinct input rows and are retained
or removed independently. For example, two matching left rows survive a semi
join as two rows even when the right side contains three copies of the key.
These joins are not deduplication operations.

An empty left input produces no semi/anti rows. An empty right input makes a
semi join empty and an anti join retain all left rows. A full join with one
empty side retains the other side, padding the absent columns with nulls.

## Nulls and order

Null keys do not match by default. Thus a null left key is excluded by semi,
retained by anti, and remains unmatched in a full join.
With `nulls_equal=True`, nulls in corresponding key positions compare equal,
including inside composite keys. Pair-producing joins still multiply
duplicate null-key matches; membership joins still retain each left row at
most once.

NaN remains a native floating-point value governed by Polars equality,
not a null. No join kind promises output order. Apply explicit sorting when
an application needs a reproducible presentation, including a tie-breaker
when keys alone are insufficient.

## Keys, names and suffixes

Inner, left and full joins retain both sides' key columns without automatic
coalescing. A right column whose name is also present on the left gets the
suffix, including keys. If this still leaves duplicate output names, the query
fails with `DataFrameError.Query`; it does not overwrite a column.

For an unmatched right row in a full join, the left key is null while the
right key keeps its value. To create a common display key, derive it explicitly:
`df.col("id")?.fill_null(df.col("id_right")?)?.alias("joined_id")?`.
Keep the originals when their provenance matters. This is a normal expression
step, not a different join coalescing policy.

Semi and anti joins emit only left columns, so they neither append right
keys nor suffix right names. The suffix argument has no effect for them,
including an empty suffix. Right-side name collisions are therefore harmless
for these membership joins.

The same expression-key and ownership rules apply to all five kinds.
Native query execution supports ordinary and explicit streaming collection;
streaming is not a promise of bounded join memory. For nearby-key matching,
use [integer as-of joins](dataframe-asof.md). Temporal/grouped as-of joins and
uniqueness-validation options are not exposed.

See [membership and full-join examples](../../tutorial/special-purpose/dataframe-membership.md),
[joining and ranking](../../tutorial/special-purpose/dataframe-joins.md),
and the [signature index](signatures.md#stddataframe).

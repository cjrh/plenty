# Clean and compare column values

Keep cleaning and derived calculations inside a query so Polars can
optimize them with the rest of the plan. These examples require the
[optional engine](../../design/stdlib/dataframe-engine.md).

## Parse values and label missing data

The explicit non-strict cast below treats invalid numeric text as missing.
That is a deliberate data-cleaning policy; omit `strict=False` when bad
input should fail the query instead.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    data = df.frame([
        df.column_str("team", ["North", "South", "NORTH"]?)?,
        df.column_str("amount", ["12", "oops", "8"]?)?
    ]?)?
    cleaned = data.lazy()?.with_columns([
        df.col("team")?.str_lower()?,
        df.col("amount")?.cast(df.DType.I64, strict=False)?
    ]?)?
    state = df.when(
        df.col("amount")?.is_null()?,
        df.lit_str("missing")?,
        df.lit_str("ready")?
    )?.alias("state")?
    result = cleaned.with_columns([state]?)?.collect()?
    print(result.values_str("team")?)?
    print(result.values_i64_nullable("amount")?)?
    print(result.values_str("state")?)?
    Ok(())
```
```output
["north", "south", "north"]
[Option[i64].Some(12), Option[i64].Nothing, Option[i64].Some(8)]
["ready", "missing", "ready"]
```

The second `with_columns` sees the parsed amount. Expressions within a
single `with_columns` all see its input schema, so separate dependent steps.
`when` is a native conditional expression, not a short-circuiting Plenty
`if`: both branches must be valid. It cannot guard a failing strict cast.

For literal matching, write
`df.col("team")?.str_contains(df.lit_str("north")?)?`.
The pattern is text, not a regular expression. A column expression can
also supply a different pattern per row.

## Compare each row with its group

A grouped aggregate produces one row per group. A window calculation
instead places the group's result alongside each original row.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    data = df.frame([
        df.column_str("team", ["north", "south", "north", "north"]?)?,
        df.column_i64("units", [2, 4, 3, 2]?)?
    ]?)?
    teams = [df.col("team")?]?
    units = df.col("units")?
    result = data.lazy()?.with_columns([
        units.sum()?.over(teams)?.alias("team_total")?,
        units.n_unique()?.over(teams)?.alias("distinct_units")?
    ]?)?.collect()?
    print(result.values_i64("team_total")?)?
    print(result.values_i64("distinct_units")?)?
    Ok(())
```
```output
[7, 4, 7, 7]
[2, 1, 2, 2]
```

`over` uses the entire partition, not a running window. It maps results
back to the original rows and does not sort them. Distinct counts include
null as one distinct value, whereas `count()` counts only present values.
Read the [expression contracts](../../design/stdlib/dataframe-expressions.md)
for casting, Unicode and null semantics.

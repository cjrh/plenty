# Types, nulls and schemas

A missing reading differs from a reading of zero. Use `Option` elements
to preserve that distinction, and keep names and flags in their native
string and boolean columns.

This example keeps enabled sensors and reads their nullable values back.
The expressions run in Polars; ordinary Plenty code handles the results.
As in the previous lesson, install the
[optional engine](../../design/stdlib/dataframe-engine.md) before running it.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    names = ["north", "café", "南"]?
    enabled = [True, False, True]?
    readings: list[Option[f64]] = [Some(18.5), Some(22.0), Nothing]?
    data = df.frame([
        df.column_str("sensor", names)?,
        df.column_bool("enabled", enabled)?,
        df.column_f64_nullable("reading", readings)?
    ]?)?
    result = data.lazy()?.filter(df.col("enabled")?)?.collect()?
    print(result.values_str("sensor")?)?
    print(result.values_f64_nullable("reading")?)?
    for field in result.schema()?:
        print(field.name, field.dtype)?
    Ok(())
```
```output
["north", "南"]
[Option[f64].Some(18.5), Option[f64].Nothing]
sensor std.dataframe.DType.String
enabled std.dataframe.DType.Bool
reading std.dataframe.DType.F64
```

`Nothing` is missing data; `Some("")`, `Some(False)`, zero and NaN are
present values. Use `values_TYPE_nullable` for nullable results.
The plain `values_TYPE` methods reject nulls rather than dropping rows.
Scalar `get_TYPE(name, row)` returns an `Option` inside a `Result`:
absence and query failure are different outcomes.

For an unfamiliar file, inspect `df.scan_parquet(path)?.schema()?` before
choosing columns or collecting. Its ordered fields describe the native
runtime schema. `DType.Other(description)` reports a native type that
Plenty cannot yet extract, such as an unsupported nested value.
Inspection reads metadata as needed; collection can still encounter errors.

## Keep numeric widths and unsigned identifiers

Every Plenty numeric type has matching column constructors and getters.
Choose the type your data needs: a `u64` identifier can exceed `i64.MAX`,
while `f32` readings use less native column memory than `f64` readings.
Neither needs a conversion through the default numeric types.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    data = df.frame([
        df.column_u64("device", [u64.MAX, 42u64]?)?,
        df.column_f32("reading", [18.5f32, 22.0f32]?)?
    ]?)?
    selected = data.lazy()?.filter(df.col("device")?.eq(df.lit_u64(u64.MAX)?)?)?.collect()?
    print(selected.values_u64("device")?)?
    print(selected.values_f32("reading")?)?
    Ok(())
```
```output
[18446744073709551615]
[18.5]
```

Use `column_TYPE_nullable` and `values_TYPE_nullable` when values may be
missing. Extraction checks the exact dtype; it does not widen integers
or floats implicitly. An expression or aggregation can change the result's
dtype, so inspect its schema or make a deliberate `cast` before extracting.

Lists are copied across the native boundary. Keep filtering, grouping and
sorting inside the query, then extract only the columns and rows your
Plenty code needs. See [the type contracts](../../design/stdlib/dataframe-types.md)
for Unicode, ownership and conversion details, then continue with
[Join and rank results](dataframe-joins.md) or
[Clean and compare column values](dataframe-expressions.md).
For logical calendar data, see [Dates and daily reports](dataframe-datetime.md).

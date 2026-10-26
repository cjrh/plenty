# Query CSV and write Parquet

Use a lazy CSV scan when Polars should filter, derive or aggregate file-backed
data. A direct Parquet sink writes the query's output without first returning
a complete dataframe to Plenty.

This example creates a small input file so it can run by itself. With real
data, begin at `scan_csv`. Install the
[optional engine](../../design/stdlib/dataframe-engine.md), and run in an
empty directory: the new output methods refuse to overwrite existing files.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    readings = df.frame([
        df.column_str("station", ["001", "002", "001"]?)?,
        df.column_f64("reading", [10.0, 20.0, 30.0]?)?
    ]?)?
    readings.write_csv("readings.csv")?
    query = df.scan_csv("readings.csv", schema_overrides=Some([
        df.Field("station", df.DType.String),
        df.Field("reading", df.DType.F64)
    ]?))?
    warm = query.filter(df.col("reading")?.gt(df.lit(15.0)?)?)?
    warm.sink_parquet("warm.parquet")?
    result = df.scan_parquet("warm.parquet")?.collect_streaming()?
    print(result.values_str("station")?)?
    print(result.values_f64("reading")?)?
    Ok(())
```
```output
["002", "001"]
[20.0, 30.0]
```

The String override keeps leading zeros in station identifiers. CSV contains
text rather than a typed schema. For unfamiliar files, inspect
`query.schema()?`. With `infer_rows=0`, unspecified columns stay strings,
so you can validate them and choose explicit casts. The default samples
100 rows; `infer_rows=-1` scans the full file for inference.

## Missing values and text

A quoted empty string and an unquoted empty field have different meanings.
The default writer/reader preserves that distinction. When copying a frame
through CSV, pass its schema to preserve column types too.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    notes: list[Option[str]] = [Some(""), Nothing, Some("雪, café")]?
    data = df.frame([df.column_str_nullable("note", notes)?]?)?
    data.write_csv("notes.csv", separator=";")?
    restored = df.scan_csv(
        "notes.csv",
        separator=";",
        schema_overrides=Some(data.schema()?)
    )?.collect_streaming()?
    print(restored.values_str_nullable("note")?)?
    Ok(())
```
```output
[Option[str].Some(""), Option[str].Nothing, Option[str].Some("雪, café")]
```

A custom null token is available through `write_csv(null_value="NA")`
and `scan_csv(null_value=Some("NA"))`. Use it only when that token cannot
be a real value: even a quoted `"NA"` is treated as null on reading.

Streaming execution does not imply a memory bound. `collect_streaming`
retains all final rows, and sorts, groups, joins or windows may buffer data.
Prefer `sink_parquet` when the desired result is a file rather than a Plenty
dataframe. A failed write can leave a partial new file; existing files are
never overwritten by these two new output methods.
See the [I/O contracts](../../design/stdlib/dataframe-io.md) for options,
error timing and execution limits.

Continue with [Application: a daily sales report](dataframe-application.md)
to combine file scans, cleaning, dates, joins and exports in a complete program.

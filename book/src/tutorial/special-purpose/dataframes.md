# Query dataframes with Polars

Use `std.dataframe` when your task is an analytical query over columns or
Parquet files. Plenty describes the query; a separately compiled Polars
engine optimizes and executes it. This exposes native columnar processing,
parallelism and SIMD without asking you to manage vector instructions.
Install the [optional engine](../../design/stdlib/dataframe-engine.md) once.
These examples require it when compiled and run.

`clist[T]` remains useful for lightweight mutable records, a compile-time
class schema and ordinary Plenty computations. A dataframe has a runtime
schema and native query semantics. It is a different tool for larger
analytical operations, not the default replacement for a collection.

## Build a query

Copy numeric lists into named columns, create a frame, and build expressions.
An expression is a native query description, not a Plenty function called
once per row. `?` handles query errors, separately from null values.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    readings = [18.0, 23.0, 29.0]?
    columns = [df.column_f64("temperature", &readings)?]?
    data = df.frame(&columns)?
    temperature = df.col("temperature")?
    warm = temperature.gt(&df.lit(20.0)?)?
    aggregates = [temperature.mean()?.alias("average")?]?
    result = data.lazy()?.filter(&warm)?.select(&aggregates)?.collect()?
    print(result.get_f64("average", 0)?)?
    Ok(())
```
```output
Option[f64].Some(26.0)
```

Column construction copies values in bulk. Plans borrow and share native
buffers; chaining operations does not copy the whole dataset at every step.
`collect()` executes the plan. A getter returns `Result[Option[T], ...]`:
`Nothing` means a null cell, while `Err` means the operation failed.
Bulk `values_f64` and `values_i64` return ordinary Plenty lists, but reject
columns containing nulls.

## Derive columns and group

Select explicit expressions and names. Group order is unspecified, so this
example prints the number of groups rather than assuming which comes first.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    stations = [1, 2, 1]?
    readings = [10.0, 20.0, 30.0]?
    columns = [df.column_i64("station", &stations)?, df.column_f64("reading", &readings)?]?
    data = df.frame(&columns)?
    derived = [df.col("reading")?.mul(&df.lit(2.0)?)?.alias("adjusted")?]?
    keys = [df.col("station")?]?
    totals = [df.col("adjusted")?.sum()?.alias("total")?]?
    result = data.lazy()?.with_columns(&derived)?.group_by(&keys, &totals)?.collect()?
    print(result.height()?)?
    data.write_parquet("readings.parquet")?
    restored = df.scan_parquet("readings.parquet")?.collect()?
    print(restored.values_f64("reading")?)?
    Ok(())
```
```output
2
[10.0, 20.0, 30.0]
```

For an existing Parquet file, start with `scan_parquet`, add filters and
projections, then collect. Polars can push those operations into the scan.
The write above creates or overwrites its file.

Native query semantics differ from Plenty scalar arithmetic: nulls and NaN
are distinct, NaN ordering differs, integer overflow is not checked like
Plenty arithmetic, and parallel floating-point reductions can change rounding.
Native allocation failure can abort the process. Read the
[reference contracts](../../design/stdlib/dataframe.md) before using these
operations where those distinctions matter.

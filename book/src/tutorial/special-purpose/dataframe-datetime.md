# Dates and daily reports

Parse a timestamp column once, then keep calendar calculations inside the
query. The example below groups events by their UTC calendar date. This is
different from splitting timestamp strings or assuming every input has an
identical textual layout.

Install the [optional engine](../../design/stdlib/dataframe-engine.md) before
running these examples. `utc=True` declares that the input text already
represents UTC; it does not convert another timezone.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    events = df.frame([
        df.column_str("text", ["2024-02-28 09:00:00", "2024-02-29 12:30:00", "2024-02-29 14:00:00"]?)?,
        df.column_i64("amount", [10, 20, 5]?)?
    ]?)?
    stamp = df.col("text")?.str_to_datetime(
        "%Y-%m-%d %H:%M:%S", df.TimeUnit.Microseconds, utc=True
    )?.alias("stamp")?
    daily = events.lazy()?.with_columns([stamp]?)?.group_by(
        [df.col("stamp")?.dt_date()?.alias("day")?]?,
        [df.col("amount")?.sum()?.alias("total")?]?
    )?.sort([df.SortKey(df.col("day")?)]?)?.select([
        df.col("day")?.dt_format("%Y-%m-%d")?.alias("day")?,
        df.col("total")?
    ]?)?.collect()?
    print(daily.values_str("day")?)?
    print(daily.values_i64("total")?)?
    Ok(())
```
```output
["2024-02-28", "2024-02-29"]
[10, 25]
```

`dt_date()` retains a logical Date while grouping and sorting; formatting
happens only for presentation. The chosen unit controls stored precision.
Microseconds keep six fractional decimal places; milliseconds keep three
and nanoseconds keep nine. Extra fractional precision is discarded toward
the earlier instant, so select the unit intentionally.

## Keep invalid input visible as missing data

Parsing is strict by default. Use `strict=False` when invalid input should
become null and your application has a policy for missing data.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    source = df.frame([
        df.column_str("text", ["1970-01-01", "1969-12-31", "2001-02-29"]?)?
    ]?)?
    dates = source.lazy()?.select([
        df.col("text")?.str_to_date(strict=False)?.alias("day")?
    ]?)?.collect()?
    print(dates.values_date_days_nullable("day")?)?
    Ok(())
```
```output
[Option[i32].Some(0), Option[i32].Some(-1), Option[i32].Nothing]
```

The raw boundary representation is days since 1970-01-01; datetimes use
integer ticks with an explicit unit. In most reports, prefer calendar
expressions and formatting to manipulating these representations yourself.
Getters check the logical dtype and, for datetimes, the requested unit and
UTC metadata. They do not silently rescale timestamps.

Use Parquet when temporal metadata should survive file round-trips. For CSV,
choose an explicit text format and parse it again on input. Named timezones,
offset parsing and leap seconds are outside this foundation. See the
[temporal contracts](../../design/stdlib/dataframe-datetime.md) for nulls,
calendar bounds, overflow and conversion costs.

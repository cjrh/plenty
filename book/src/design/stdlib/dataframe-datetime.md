# Dates and datetimes

Temporal columns have logical types, not merely integer labels. Their
construction and extraction use explicit physical units at the Plenty boundary;
native parsing and calendar expressions handle ordinary analytical workflows.
All operations return `Result[..., DataFrameError]`.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    source = df.frame([df.column_str("text", ["2024-02-29", "1970-01-02"]?)?]?)?
    day = df.col("day")?
    result = source.lazy()?.select([
        df.col("text")?.str_to_date()?.alias("day")?
    ]?)?.with_columns([
        day.dt_year()?.alias("year")?,
        day.dt_format("%d/%m")?.alias("label")?
    ]?)?.collect()?
    print(result.values_date_days("day")?)?
    print(result.values_i64("year")?)?
    print(result.values_str("label")?)?
    Ok(())
```
```output
[19782, 1]
[2024, 1970]
["29/02", "02/01"]
```

## Units, timezones and the boundary

| Logical type | Construction | Extraction |
|---|---|---|
| Date | `column_date(name, days: &list[i32])` | `values_date_days(name)` / `get_date_days(name, row)` |
| Datetime | `column_datetime(name, ticks: &list[i64], unit, utc=False)` | `values_datetime_ticks(name, unit, utc=False)` / `get_datetime_ticks(name, row, unit, utc=False)` |

Dates count proleptic Gregorian calendar days since 1970-01-01. Datetimes count
ticks from 1970-01-01 00:00:00. `TimeUnit.Milliseconds`, `Microseconds` and
`Nanoseconds` mean 1,000, 1,000,000 and 1,000,000,000 ticks per second.
Negative values precede the epoch. No unit is inferred or converted by a getter.
Date extraction returns `i32`; datetime extraction returns `i64`.

`utc=False` describes a timezone-free civil datetime. `utc=True` describes
UTC; it does not convert a local wall-clock value into UTC. No host timezone
is consulted. Named-zone localization, timezone conversion, daylight-saving
ambiguities and leap seconds are not supported in this foundation.
[`std.time.Timestamp`](time.md#wall-clock-timestamps) uses seconds plus
nanoseconds and is not implicitly converted to these column representations.

Each constructor also has a `_nullable` form accepting `list[Option[T]]`.
Each bulk getter has a `_nullable` form returning `list[Option[T]]`; the plain
getter rejects any null with `DataFrameError.Null`. Scalar getters return
`Option[T]` inside `Result`. Empty and all-null columns retain their dtype.
Transfers copy whole buffers; nullable values use a separate validity buffer.

Construction and typed extraction validate the engine's representable calendar
range. Not every `i32` day or `i64` millisecond/microsecond value is a valid
calendar date. The full `i64` nanosecond range is representable. Invalid values
produce errors rather than wrapping; null payload bits are ignored.
Wrong dtype, unit or UTC metadata is an error, not an implicit conversion.

Schemas report `DType.Date` or `DType.Datetime(unit, timezone: Option[str])`.
`Nothing` is naive and `Some("UTC")` is UTC. Other timezone labels from files
are preserved honestly, but the typed temporal getters and calendar operations
reject them. Schema inspection is not a promise that every reported type or
value can be extracted. Generic `get_i64` does not discard datetime metadata.

## Parse strings and extract calendar fields

| Expression method | Output |
|---|---|
| `str_to_date(format="%Y-%m-%d", strict=True)` | Date |
| `str_to_datetime(format, unit, utc=False, strict=True)` | Datetime with the specified unit/zone |
| `dt_year()`, `dt_month()`, `dt_day()` | I64 calendar component |
| `dt_hour()`, `dt_minute()`, `dt_second()` | I64 time component of a Datetime |
| `dt_date()` | Date portion of a Datetime |
| `dt_format(format)` | String |

Formats use Chrono directives such as `%Y-%m-%d`, `%H:%M:%S` and `%.f`
(optional fractional seconds). Parsing consumes the whole string. Omitted
clock fields default to zero. Invalid values and out-of-range conversions
fail with `Query` in strict mode or become null with `strict=False`.
Existing nulls remain null. Parsing leap-second `:60` is rejected.

Fractional precision beyond the selected unit is discarded toward the earlier
instant: `-1` nanosecond becomes `-1` millisecond, not zero. Choose nanoseconds
when that precision matters; their calendar range is much smaller than that
of milliseconds or microseconds. Nanosecond overflow is checked explicitly.

Invalid formats and timezone directives (`%z`, `%Z`, `%+`, including modified
forms) are rejected. A literal `Z` is just text; use it only when the data is
explicitly UTC. Date-only values cannot supply time-of-day formatting fields.
Unicode literals in formats are preserved. Calendar components use the stored
naive/UTC interpretation and return null for null inputs.

Parsing and range checks execute as checked native column expressions inside
Polars plans, with no Plenty callback or per-cell FFI. These validation/parsing
nodes are opaque to the optimizer; calendar kernels remain native Polars
operations. The checked parser avoids overflow and leap-second normalization
in the pinned native parser. Imported invalid calendar values fail explicitly
before calendar extraction or formatting.

Parquet preserves temporal dtypes and metadata. CSV does not: format columns
explicitly before writing and parse the resulting strings after reading.
CSV schema overrides and `Expr.cast` accept numeric, boolean and string target
types; they do not construct temporal types. As-of joins remain integer-key
operations. See the [runnable lesson](../../tutorial/special-purpose/dataframe-datetime.md)
and [signature index](signatures.md#stddataframe).

## Acknowledgements

- Date parsing, calendar validation and format directives use [Chrono](https://github.com/chronotope/chrono) 0.4.45, built into the dataframe engine (MIT OR Apache-2.0).

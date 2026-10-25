# Match nearby observations

An equality join requires identical keys. An **as-of join** instead finds the
most recent, next or nearest observation. This is useful for attaching a
calibration value to sensor readings or a price to an event.

Use integer keys with a unit your application defines. Here, keys represent
elapsed seconds. They are ordinary `i64` values, not a date/time type.
These examples require the [optional dataframe engine](../../design/stdlib/dataframe-engine.md).
As-of validation materializes both inputs when the query executes, including
with `collect_streaming`; filter and select the needed columns beforehand
when working with larger data.

## Use the most recent calibration

The source below is already ordered. For unsorted data, sort both query inputs
by their key before joining. The engine checks ascending order rather than
silently producing a misleading match.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    readings = df.frame([
        df.column_i64("second", [2, 6, 12]?)?,
        df.column_f64("reading", [10.0, 20.0, 30.0]?)?
    ]?)?
    calibration = df.frame([
        df.column_i64("second", [1, 5]?)?,
        df.column_f64("offset", [0.5, 1.5]?)?
    ]?)?
    joined = readings.lazy()?.join_asof(
        calibration.lazy()?, "second", "second", tolerance=Some(3)
    )?
    corrected = df.col("reading")?.add(df.col("offset")?)?.alias("corrected")?
    result = joined.with_columns([corrected]?)?.collect()?
    print(result.values_f64_nullable("corrected")?)?
    Ok(())
```
```output
[Option[f64].Some(10.5), Option[f64].Some(21.5), Option[f64].Nothing]
```

Backward matching is the default: calibration at second 5 can apply to a
reading at second 6, but cannot apply to a reading at second 2. The final
reading has no calibration within three seconds; its null offset propagates
through addition. This is missing data, not a query failure. Both original
key columns remain available as `second` and `second_right` for inspection.

## Choose the nearest observation

Nearest matching can look into the future. Choose it deliberately when that
is meaningful for your data; do not use it for a causal lookup by accident.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    events = df.frame([df.column_i64("second", [2, 4]?)?]?)?
    samples = df.frame([
        df.column_i64("sample_second", [1, 3, 5]?)?,
        df.column_str("label", ["early", "middle", "late"]?)?
    ]?)?
    result = events.lazy()?.join_asof(
        samples.lazy()?, "second", "sample_second",
        strategy=df.AsOfStrategy.Nearest, tolerance=Some(1)
    )?.collect()?
    print(result.values_str("label")?)?
    Ok(())
```
```output
["middle", "late"]
```

Each event is equally far from two samples; nearest matching selects the
greater key. `AsOfStrategy.Forward` always chooses the next qualifying key.
For backward/forward matching, set `allow_exact_matches=False` when a strictly
earlier/later sample is required. See [as-of join contracts](../../design/stdlib/dataframe-asof.md)
for duplicate keys, nulls, limits and failure behavior.

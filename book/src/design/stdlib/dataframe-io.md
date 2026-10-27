# CSV and streaming dataframe I/O

These operations use the [optional Polars engine](dataframe-engine.md).
They return `Result[..., DataFrameError]`, retain native error messages,
and leave query/frame owners usable after each call.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    data = df.frame([
        df.column_str("city", ["Oslo", "Lima", "Pune"]?)?,
        df.column_i64("population", [709, 10883, 7764]?)?
    ]?)?
    data.write_csv("cities.csv")?
    query = df.scan_csv("cities.csv", schema_overrides=Some(data.schema()?))?
    large = query.filter(df.col("population")?.gt(df.lit_i64(1000)?)?)?.collect()?
    print(large.values_str("city")?)?
    Ok(())
```
```output
["Lima", "Pune"]
```

## Scan a local CSV file

`scan_csv(path, has_header=True, separator=",", infer_rows=100,
null_value=Nothing, schema_overrides=Nothing)` returns a `Query`.
The path names one file; wildcard characters are literal, not a glob.

| Option | Contract |
|---|---|
| `has_header: bool` | First record supplies names; false uses `column_1`, `column_2`, … |
| `separator: &str` | One ASCII byte, excluding NUL, double quote, CR and LF; tab is allowed |
| `infer_rows: i64` | Positive: sample that many rows; zero: infer strings; `-1`: inspect the whole file |
| `null_value: Option[str]` | One optional exact token treated as null in every column |
| `schema_overrides: Option[list[Field]]` | Named overrides using any numeric `DType`, `Bool` or `String`; other columns retain inference |

Overrides reuse [schema fields](dataframe-types.md). The optional list is
consumed, so `schema_overrides=Some(frame.schema()?)` is convenient for
CSV round-trips that must preserve these scalar override types. Duplicate or
unknown override names, temporal targets and `DType.Other` fail with `Schema`.
For [temporal columns](dataframe-datetime.md), explicitly format to strings
before CSV output and parse those strings after input; Parquet preserves their metadata.
Overrides select types by name; they are not a full schema or a column
rename/reorder facility. Headerless empty input has no width/names to infer.

CSV carries text, not a durable typed schema. Quoting does not guarantee
that an inferred column stays a string; specify a String override for
identifiers such as `"001"`. A sample can infer an integer and then fail
on a later non-integer value. Use overrides, increase the sample, or read
strings and [cast explicitly](dataframe-expressions.md#explicit-casts).
Whole-file inference costs a full scan before query execution.

Input must be valid UTF-8. Double quotes delimit quoted fields and doubled
quotes escape quote characters. Quoted separators and newlines are supported.
Malformed records, invalid typed values and invalid UTF-8 produce errors;
the facade does not enable lossy decoding, ignored parse errors or ragged-row
truncation. A scan may inspect schema/sample data immediately; errors can
occur during creation, schema inspection or later execution. The file must
remain available while the query executes.

## Empty strings and null tokens

By default, an unquoted empty field is null; a quoted empty **string** field
is present empty text. This preserves `Nothing` versus `Some("")` in
typed string round-trips. Floating NaN remains a present floating value.

A custom null token also matches quoted fields: with `null_value=Some("NA")`,
both `NA` and `"NA"` become null. Choose a token that does not occur as a
real value. CSV quoting cannot disambiguate this collision. Empty/missing
fields still follow the native missing-is-null policy.

## Write a collected frame as CSV

`frame.write_csv(path, has_header=True, separator=",", null_value="")`
returns `Result[(), DataFrameError]`. It writes a new local UTF-8 file,
with double-quote escaping and LF record endings. This starts from an
already collected dataframe; it is not a query sink.

The output null token is written verbatim. It cannot contain the separator,
double quote, CR or LF; invalid options fail before creating a file.
The empty default writes null as an unquoted empty field and preserves
present empty strings through quoting. Read a custom token back by passing
the same token to `scan_csv`. CSV does not preserve types by itself.

## Execute with the streaming engine

`query.collect_streaming()` returns a `DataFrame` and explicitly selects
Polars' streaming engine. It still materializes the **entire final result**.
It is useful when streaming intermediate work helps, not when the final
result itself cannot fit in memory. Ordinary `collect()` retains its native
automatic execution policy.

`query.sink_parquet(path)` executes a native Parquet file-sink plan using
the streaming engine and returns `()`. The wrapper does not collect a
complete dataframe and then call a writer. The opened output file is passed
directly to Polars, and intermediate rows remain inside the native plan.
For example, a CSV scan can filter/project and write Parquet without
converting rows into Plenty values or retaining the whole output frame.
Default native Parquet compression and row-group settings apply.

Neither method guarantees constant or bounded memory for arbitrary plans.
Sorts, joins, high-cardinality groups, distinct counts and partition windows
can buffer substantial data, and the streaming planner can use in-memory
stages for some operations. A lazy frame built from Plenty columns already
holds its source data in memory. Unsupported operations or resource failures
can still error, and native OOM can abort. No batch iterator, memory budget,
spill-directory control or cloud configuration is exposed by this increment.

The sink preserves the plan's order where defined; it does not make unordered
group/join output deterministic. Sort explicitly when output order matters,
accepting the associated buffering cost.

## Output ownership and failures

The new `write_csv` and `sink_parquet` methods **create a new file**.
An existing path, including a symlink or an alias of a lazy input, fails with
`Io` without truncation. Parent directories must already exist.
The sink keeps the opened file descriptor through execution instead of
reopening its path. Neither method mutates or consumes the input owner.

After creation, an I/O or query error may leave a partial new file. These
methods do not provide atomic publication, automatic cleanup or an fsync
durability guarantee. Choose a fresh output name; explicitly manage cleanup
or publication in the application when necessary.
The older `DataFrame.write_parquet` retains its documented overwrite behavior.

Native Polars failures use `DataFrameError.Query`; direct file creation
failures use `Io`, and invalid facade options use `Schema`. Other native
failure and allocation contracts are unchanged.

See the [runnable lesson](../../tutorial/special-purpose/dataframe-io.md).
For row-oriented parsing over Plenty streams without Polars, use
[`std.csv`](csv.md).

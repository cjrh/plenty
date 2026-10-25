# `std.dataframe`

Polars-backed analytical dataframes, expression plans and Parquet files.
The optional, separately compiled engine is reused by every Plenty program.
See [installation and deployment](dataframe-engine.md) and the
[tutorial](../../tutorial/special-purpose/dataframes.md).

Use `clist[T]` for a lightweight mutable columnar collection with a
compile-time class schema and Plenty arithmetic. Use `std.dataframe` for
analytical queries and file-backed datasets with runtime schemas. Neither
replaces ordinary lists.

## Construction and extraction

`column_i64(name: &str, values: &list[i64])` and
`column_f64(name: &str, values: &list[f64])` return
`Result[Column, DataFrameError]`. Each makes one native call and copies the
borrowed numeric values into a native contiguous column. Names are copied;
inputs remain usable. No input pointer survives the call.

`frame(columns: &list[Column]) -> Result[DataFrame, DataFrameError]`
shares the columns' native buffers. Duplicate names and unequal lengths fail.
An empty column list creates a zero-row, zero-column frame. Zero-length
typed columns retain their types. This engine supports at most
`2**32 - 1` rows.

`DataFrame.height()` returns `Result[i64, DataFrameError]`.
`get_i64(name, row)` and `get_f64(name, row)` return
`Result[Option[i64], DataFrameError]` and
`Result[Option[f64], DataFrameError]`. Bounds, missing names and mismatched
types fail; a valid null cell returns `Ok(Nothing)`.

`values_i64(name)` and `values_f64(name)` return a Plenty list in a
`Result`. They allocate initialized Plenty storage, then copy the whole
column in one native call. A column containing any nulls fails with
`DataFrameError.Null`; use scalar nullable extraction or `fill_null`.
There is no implicit numeric cast, zero-copy borrowing or nullable bulk
input/output in this foundation.

## Expression plans

Every operation below returns a `Result[..., DataFrameError]`.
The [signature index](signatures.md#stddataframe) gives exact declarations.

| Operation | Result value |
|---|---|
| `col(name)`, `lit(f64)`, `lit_i64(i64)`, `null_f64()` | `Expr` |
| `expr.add/sub/mul/div/gt/eq/lt/ge/le(&other)` | `Expr` |
| `expr.and_(&other)`, `or_(&other)`, `fill_null(&other)` | `Expr` |
| `expr.sum/count/mean/min/max/is_null()` | `Expr` |
| `expr.alias(name)` | `Expr` |
| `frame.lazy()`, `scan_parquet(path)` | `Query` |
| `query.filter(&predicate)` | `Query` |
| `query.with_columns(&expressions)`, `select(&expressions)` | `Query` |
| `query.group_by(&keys, &aggregates)` | `Query` |
| `query.collect()` | `DataFrame` |
| `frame.write_parquet(path)` | `()` |

Expressions describe native Polars operations; they are not opaque Plenty
callbacks. Polars can optimize a complete plan, push projections/filters
into Parquet scans, and use its worker pool. A scan is lazy; errors can
arise during construction or collection. `with_columns` replaces matching
names or adds new columns. Expressions in one call see the input schema;
chain calls when a later expression depends on a newly derived column.
Selection projects or aggregates. Grouping supports multiple keys and
aggregates, but does not promise group order. Collect and sort externally
when an application needs a particular order.

Owners and plans are immutable through this public API. Methods borrow
their receivers; resulting plans clone native handles and share buffers.
Dropping the original columns, frame or expressions does not invalidate a
derived query. Each handle is freed by the engine that allocated it.
There are no public raw pointers or user-created native handles.

## Semantics and limits

Polars 0.55.2 semantics apply inside expressions, not Plenty scalar
semantics. Schemas are checked at runtime. Input/extraction supports only
`i64` and `f64`; Parquet can contain other native types that expressions
can carry, but this API cannot extract them. Paths are UTF-8 strings.
There is no automatic lambda translation, mutation API, join, string/date
construction, cloud credential configuration or stable serialization of plans.

Null is missing data, distinct from floating-point NaN. Comparisons with
null yield null, which filtering drops. Polars orders NaN above finite
numbers: `NaN > 50.0` may keep a row that a Plenty scalar comparison would
drop. Aggregations generally skip nulls; count counts non-null values and
returns `i64`. Floating-point sums may differ with execution order and
parallelism. Group order is unspecified. Native integer arithmetic and
reductions are not Plenty's checked arithmetic: overflow must not be
assumed to return a recoverable error. Validate input ranges where needed.

`DataFrameError` retains native messages in `Schema`, `Query`, `Io`,
`NativePanic`, `Null`, and `Bounds`. Polars errors, including some file
errors, use `Query`; direct filesystem errors use `Io`. Converting an
error message can itself fail with `Allocation(AllocError)` or
`Message(IoError)`. Plenty-side list allocation is recoverable.
**Native Rust/Polars allocation failure can abort the process.**
Caught Rust panics become errors where unwinding is available; this does
not make arbitrary native failures recoverable.

`POLARS_MAX_THREADS` configures the engine's global pool before first use.
It is independent of Plenty executors. Avoid oversubscribing both pools
when running many analytical queries concurrently.

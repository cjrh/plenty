# `std.dataframe`

Polars-backed analytical dataframes, expression plans and CSV/Parquet files.
The optional, separately compiled engine is reused by every Plenty program.
See [installation and deployment](dataframe-engine.md) and the
[tutorial](../../tutorial/special-purpose/dataframes.md).
The [daily sales report application](../../tutorial/special-purpose/dataframe-application.md)
combines ingestion, checked catalogue joins, row quarantine and typed exports.

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
`DataFrameError.Null`; use `values_i64_nullable` or `values_f64_nullable`
to preserve missing values, or `fill_null` when replacement is appropriate.
Strings, booleans, binary payloads, nullable input/output and schema inspection are described
in [Column types and schemas](dataframe-types.md). No implicit casts or
zero-copy borrowing occur at the Plenty boundary.
Logical [Date and Datetime columns](dataframe-datetime.md) add checked temporal
interchange, parsing, formatting and calendar expressions.

## Expression plans

Every operation below returns a `Result[..., DataFrameError]`.
The [signature index](signatures.md#stddataframe) gives exact declarations.

| Operation | Result value |
|---|---|
| `col(name)`, `lit(f64)`, `lit_i64(i64)`, `lit_str(text)`, `lit_bool(value)`, `lit_binary(payload)`, `null_f64()` | `Expr` |
| `when(predicate, then_value, otherwise)`, `expr.cast(dtype, strict=True)` | `Expr` |
| `expr.str_lower/str_upper/str_len()`, `str_contains/str_starts_with/str_ends_with(pattern)` | `Expr` |
| `expr.n_unique()`, `expr.over(partition_by)` | `Expr` |
| `expr.add/sub/mul/div/gt/eq/lt/ge/le(&other)` | `Expr` |
| `expr.and_(&other)`, `or_(&other)`, `fill_null(&other)` | `Expr` |
| `expr.sum/count/mean/min/max/is_null()` | `Expr` |
| `expr.alias(name)` | `Expr` |
| `frame.lazy()`, `scan_parquet(path)` | `Query` |
| `query.filter(&predicate)` | `Query` |
| `query.with_columns(&expressions)`, `select(&expressions)` | `Query` |
| `query.group_by(&keys, &aggregates)` | `Query` |
| `query.join(&other, &left_keys, &right_keys, how=JoinKind.Inner, suffix="_right", nulls_equal=False)` | `Query` |
| `query.join_asof(&other, "key", "key", strategy=AsOfStrategy.Backward, tolerance=Nothing, allow_exact_matches=True, suffix="_right")` | `Query` |
| `query.sort(&keys)`, `slice(offset, length)`, `limit(count)` | `Query` |
| `query.collect()`, `collect_streaming()` | `DataFrame` |
| `scan_csv(path, ...)` | `Query` |
| `frame.write_csv(path, ...)`, `query.sink_parquet(path)` | `()` |
| `frame.write_parquet(path)` | `()` |

See [expression contracts](dataframe-expressions.md) for conditional evaluation,
explicit casts, string operations, distinct counts and partitioned windows.

Expressions describe native Polars operations; they are not opaque Plenty
callbacks. Polars can optimize a complete plan, push projections/filters
into Parquet scans, and use its worker pool. A scan is lazy; errors can
arise during construction or collection. `with_columns` replaces matching
names or adds new columns. Expressions in one call see the input schema;
chain calls when a later expression depends on a newly derived column.
Selection projects or aggregates. Grouping supports multiple keys and
aggregates, but does not promise group order. Add `sort` to the plan when
an application needs a particular order.

Owners and plans are immutable through this public API. Methods borrow
their receivers; resulting plans clone native handles and share buffers.
Dropping the original columns, frame or expressions does not invalidate a
derived query. Each handle is freed by the engine that allocated it.
There are no public raw pointers or user-created native handles.

## Joins, ordering and row windows

For sorted integer keys, [as-of joins](dataframe-asof.md) attach the previous,
next or nearest right row with an optional distance bound. Unlike equality
joins, they produce at most one match for each left row.

These operations return new immutable query plans; neither input is consumed
or mutated. They execute inside Polars, with no Plenty row callbacks or
intermediate conversion into Plenty lists. Input owners, key expressions and
sort specifications may be dropped after constructing the resulting plan.

`JoinKind` offers `Inner`, `Left`, `Full`, `Semi` and `Anti` equality joins.
Inner/left/full retain both sets of key columns; semi/anti are membership
queries returning only left columns. See [equality join contracts](dataframe-joins.md)
for duplicate/null matching, key retention, suffix collisions and validation.

`SortKey(expression: Expr, descending: bool = False, nulls_last: bool = True)`
owns its expression; constructing the wrapper itself is infallible.
`query.sort(keys: &list[SortKey])` orders lexicographically, with the first
key most significant. Every key has independent direction and null placement.
Null placement is independent of direction: `nulls_last=True` puts nulls last
even in descending order. NaN follows Polars value ordering. Equal keys retain
their input order; that does not make ties from an unordered join/group
deterministic. Include an additional identifying key when that matters.
An empty sort list fails with `Schema`.

`slice(offset: i64, length: i64)` selects a row window.
The second argument is a **count**, unlike Plenty list `slice(start, stop)`.
Negative offsets count from the end. The requested interval is intersected
with the available rows: `slice(-2, 10)` keeps the last two rows, while
`slice(-100, 2)` on a three-row frame is empty. Offsets beyond the end also
produce empty frames. `length` must be in `0..4294967295` inclusive;
negative or larger lengths fail immediately with `DataFrameError.Bounds`.
`limit(count)` is `slice(0, count)`, with the same length checks.
Sort first when a page or top-N result needs a defined ordering.

See [CSV and streaming I/O](dataframe-io.md) for file options, direct sinks,
create-new output behavior and buffering limits.

## Semantics and limits

Polars 0.55.2 semantics apply inside expressions, not Plenty scalar
semantics. Schemas are checked at runtime. Input/extraction supports
all Plenty numeric widths (`i8/i16/i32/i64`, `u8/u16/u32/u64`, `f32/f64`),
`str`, `bool`, Binary (`std.bytes.Bytes`), Date and naive/UTC Datetime,
including nullable columns. Parquet can
contain other native types that schema inspection describes and expressions
can carry, but this API cannot extract them. Paths are UTF-8 strings.
Lists and structs have [column-oriented construction and extraction](dataframe-nested.md),
native access expressions and structured schema inspection. There is no automatic
lambda translation, mutation API, cloud credential configuration or stable
serialization of plans.

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

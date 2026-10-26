# Dataframe column types and schemas

The [optional dataframe engine](dataframe-engine.md) supports these boundary
types. Construction and extraction return `Result[..., DataFrameError]`;
names and dtypes are checked at runtime. No implicit cast is performed.

| Column dtype | Non-nullable constructor | Non-nullable extraction |
|---|---|---|
| `i64` | `column_i64(name, values)` | `frame.values_i64(name)` |
| `f64` | `column_f64(name, values)` | `frame.values_f64(name)` |
| `str` | `column_str(name, values)` | `frame.values_str(name)` |
| `bool` | `column_bool(name, values)` | `frame.values_bool(name)` |

Logical [Date and Datetime columns](dataframe-datetime.md) have separate checked
constructors/getters with explicit epoch-day or timestamp-unit interchange.

Constructors borrow `list[T]`; extraction returns an owned `list[T]`.
All four also have `column_TYPE_nullable(name, &list[Option[T]])` and
`frame.values_TYPE_nullable(name) -> Result[list[Option[T]], DataFrameError]`.
Use these when nulls are meaningful. Non-nullable extraction fails with
`DataFrameError.Null` if any cell is null, even if the column was originally
constructed without nulls and a later join introduced them.

`get_i64`, `get_f64`, `get_str` and `get_bool` accept a column name and
zero-based row index and return `Result[Option[T], DataFrameError]`.
A null cell is `Ok(Nothing)`; an unknown name, wrong dtype or invalid index
is an error. Empty and all-null typed columns retain their declared dtype.
Native columns support at most `2**32 - 1` rows. String input currently
has the slightly lower limit `2**32 - 2`: its offset buffer needs one
entry beyond the number of rows.

## Text, booleans and missing values

Strings are UTF-8, preserving Unicode scalars and embedded NUL bytes.
There is no normalization or locale conversion. Empty text is `Some("")`,
not `Nothing`. Booleans remain booleans; they are not implicitly converted
to integers. Floating-point NaN and infinity remain present floating-point
values. In particular, `Some(f64.NAN)` does not become null.

Both inputs and outputs own their data independently of the other side.
Constructing a column never retains pointers into a Plenty list.
A query may outlive its original frame, and extracted lists may outlive
the native frame or query. Native owners are released by their allocating
engine.

Existing non-nullable numeric paths transfer a whole column in one native
call. Nullable numeric and boolean paths pack scalar values and validity
bytes, then transfer whole buffers. String input joins UTF-8 text and
constructs byte offsets and validity; output takes an owned native snapshot,
copies packed bytes/offsets/validity in bulk and decodes each byte range
into an ordinary Plenty string. This is **copying**, with temporary buffers
and additional per-string allocations, not Arrow zero-copy interchange.
It avoids per-cell foreign calls, but conversion can dominate small queries.
All these allocations remain subject to the [native OOM limitation](dataframe.md#semantics-and-limits).

## Inspect a schema before collecting

`DataFrame.schema()` and `Query.schema()` return
`Result[list[Field], DataFrameError]`, in column order.
Each `Field` has public `name: str` and `dtype: DType` fields.

`DType` has `I64`, `F64`, `Bool`, `String`, `Date`,
`Datetime(TimeUnit, Option[str])`, `List`, `Struct` and `Other(str)` variants.
Datetime metadata identifies its unit and optional timezone label; construction
and extraction currently support naive and UTC values only.
`Other(description)` preserves the engine's native dtype description,
including unexposed numeric or fixed-size array types. It does not imply that
a type can be extracted or that the description is a stable serialization
format. Do not parse that text to manufacture a cast.

Query schema inspection resolves the native plan without collecting all
rows. A Parquet scan may read file metadata, and invalid paths, missing
columns or incompatible expressions may fail at this stage. It is not a
promise that later collection will succeed: data and I/O failures can
still occur. Inspection returns independent metadata and does not mutate
or consume the query.

See the [runnable lesson](../../tutorial/special-purpose/dataframe-types.md)
and the [signature index](signatures.md#stddataframe).

[Nested columns](dataframe-nested.md) add `List` and `Struct` dtype variants,
column-oriented construction/extraction and structured `schema_tree()` inspection.

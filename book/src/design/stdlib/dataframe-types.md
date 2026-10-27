# Dataframe column types and schemas

The [optional dataframe engine](dataframe-engine.md) supports these boundary
types. Construction and extraction return `Result[..., DataFrameError]`;
names and dtypes are checked at runtime. No implicit cast is performed.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    labels: list[Option[str]] = [Some("low"), Nothing]?
    data = df.frame([
        df.column_u8("level", [3u8, 200u8]?)?,
        df.column_str_nullable("label", labels)?
    ]?)?
    for field in data.schema()?:
        print(field.name, field.dtype)?
    print(data.values_u8("level")?)?
    print(data.values_str_nullable("label")?)?
    print(data.get_str("label", 1)?)?
    Ok(())
```
```output
level std.dataframe.DType.U8
label std.dataframe.DType.String
[3, 200]
[Option[str].Some("low"), Option[str].Nothing]
Option[str].Nothing
```

| Column dtype | Non-nullable constructor | Non-nullable extraction |
|---|---|---|
| `i8`, `i16`, `i32` | `column_i8/i16/i32(name, values)` | `frame.values_i8/i16/i32(name)` |
| `i64` | `column_i64(name, values)` | `frame.values_i64(name)` |
| `u8`, `u16`, `u32`, `u64` | `column_u8/u16/u32/u64(name, values)` | `frame.values_u8/u16/u32/u64(name)` |
| `f32` | `column_f32(name, values)` | `frame.values_f32(name)` |
| `f64` | `column_f64(name, values)` | `frame.values_f64(name)` |
| `str` | `column_str(name, values)` | `frame.values_str(name)` |
| `bool` | `column_bool(name, values)` | `frame.values_bool(name)` |
| Binary (`std.bytes.Bytes`) | `column_binary(name, values)` | `frame.values_binary(name)` |

Logical [Date and Datetime columns](dataframe-datetime.md) have separate checked
constructors/getters with explicit epoch-day or timestamp-unit interchange.

Constructors borrow `list[T]`; extraction returns an owned `list[T]`.
All scalar types also have `column_TYPE_nullable(name, &list[Option[T]])` and
`frame.values_TYPE_nullable(name) -> Result[list[Option[T]], DataFrameError]`.
Use these when nulls are meaningful. Non-nullable extraction fails with
`DataFrameError.Null` if any cell is null, even if the column was originally
constructed without nulls and a later join introduced them.

`get_TYPE` (for each numeric type, `str`, `bool` and binary) accepts a column name and
zero-based row index and returns `Result[Option[T], DataFrameError]`.
A null cell is `Ok(Nothing)`; an unknown name, wrong dtype or invalid index
is an error. Empty and all-null typed columns retain their declared dtype.
Native columns support at most `2**32 - 1` rows. String input currently
has the slightly lower limit `2**32 - 2`: its offset buffer needs one
entry beyond the number of rows. Binary input has the same offset limit and
currently accepts at most `2**32 - 1` packed payload bytes per call.

Numeric input, extraction and Parquet round trips preserve signedness and
width. In particular, `u64.MAX` never passes through an `i64` or a float.
Extraction requires the exact dtype: use an explicit expression cast when
conversion is intended. Native operations can change a result dtype (for
example, a reduction may widen a narrow integer); inspect the result schema
and choose its matching getter. Polars arithmetic and overflow semantics still
apply inside queries.

`lit_i8/i16/i32/i64`, `lit_u8/u16/u32/u64` and `lit_f32` create expressions
with those exact types; `lit(value)` remains the `f64` literal constructor.

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

## Binary payloads

Binary columns contain arbitrary byte sequences, including embedded NUL and
invalid UTF-8. Their Plenty values are owned [`std.bytes.Bytes`](bytes.md)
buffers. `Some(empty_buffer)` is a present zero-byte payload; `Nothing` is
missing data. Constructors borrow buffers and copy their contents. Extraction
returns independent mutable buffers; changing them cannot change the frame.

`lit_binary(value: &Bytes)` copies a constant payload into a native expression.
Use it with equality, filtering, or `fill_null`. `cast(DType.Binary)` explicitly
converts strings to UTF-8 bytes. Binary-to-string casts require valid UTF-8;
the pinned engine rejects invalid sequences even with `strict=False`.
Binary columns round-trip
through Parquet, but are not supported by the CSV interface.

Transfers pack bytes, offsets, and validity into whole-column native calls.
The current foreign interface stages payload bytes in packed `list[u8]` buffers. Extraction
also copies through a packed `Bytes` before allocating each result buffer.
These are copying APIs with temporary memory overhead, not zero-copy views;
select/filter in Polars before extracting large payloads.

See the [binary-column lesson](../../tutorial/special-purpose/dataframe-binary.md).

## Inspect a schema before collecting

`DataFrame.schema()` and `Query.schema()` return
`Result[list[Field], DataFrameError]`, in column order.
Each `Field` has public `name: str` and `dtype: DType` fields.

`DType` has `I8`, `I16`, `I32`, `I64`, `U8`, `U16`, `U32`, `U64`,
`F32`, `F64`, `Bool`, `String`, `Binary`, `Date`,
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

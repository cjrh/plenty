# Nested dataframe columns

Lists and structs compose from existing native columns. They do not require a
dynamic Plenty object for every row. Constructors borrow their inputs; returned
columns, extracted parts and query plans remain usable after those inputs drop.
All operations below return `Result[..., DataFrameError]`.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    items = df.column_i64("item", [1, 2, 3, 4, 5]?)?
    # Offsets split the items into rows [1, 2, 3], [] and [4, 5].
    baskets = df.column_list("basket", items, [0, 3, 3, 5]?)?
    report = baskets.to_frame()?.lazy()?.select([
        df.col("basket")?.list_len()?.alias("count")?,
        df.col("basket")?.list_get(0)?.alias("first")?
    ]?)?.collect()?
    print(report.values_i64("count")?)?
    print(report.values_i64_nullable("first")?)?
    Ok(())
```
```output
[3, 0, 2]
[Option[i64].Some(1), Option[i64].Nothing, Option[i64].Some(4)]
```

## Construct lists and structs

`column_list(name, values, offsets)` groups a flattened child `Column` into
variable-length lists. For `n` rows, `offsets` contains `n + 1` integers: it
starts at zero, never decreases, and ends at the child column length. Row `i`
contains child values in `[offsets[i], offsets[i + 1])`. Equal offsets make an
empty list. The child dtype, including nested types, is preserved even when
the child is empty or entirely null. Its column name is not part of the list
element schema.

`column_list_nullable(name, values, offsets, valid)` adds a `list[bool]` with
one entry per parent row. A false entry means a null list, not an empty list.
Null rows must have equal consecutive offsets; hidden children are rejected.
Child nulls are independent: a present list can contain null elements.
The non-nullable constructor only promises present parent lists.
The extra offset entry limits list construction to `2**32 - 2` parent rows;
the flattened child retains the engine's `2**32 - 1` row limit.

`column_struct(name, fields)` takes a child `DataFrame`. Its column names,
order and dtypes become the struct fields, and its height becomes the parent
row count. At least one field is required. Zero-row typed structs work.
`column_struct_nullable(name, fields, valid)` takes one boolean per parent row.
Null parents mask their fields; values underneath null parents are not retained
as meaningful data. A present struct with all-null fields remains distinct
from a null struct.

Input lists and validity buffers are copied into native storage. Struct fields
can share immutable native buffers. Constructors validate lengths and offsets;
malformed input returns an error rather than indexing unchecked memory.

## Extract column-oriented parts

`frame.column(name)` returns an independent `Column` handle sharing native
buffers. `column.to_frame()` wraps it in a single-column dataframe with its
existing name; use that frame's ordinary typed extraction methods when values
must enter Plenty. `column.len()` returns its row count.

`column.list_parts()` returns `ListParts` with:

| Field | Meaning |
|---|---|
| `values: Column` | Flattened child values, named `item` |
| `offsets: list[i64]` | Canonical offsets starting at zero |
| `valid: list[bool]` | Parent presence, independently of child nulls |

Null parent ranges are empty. Extraction may compact/rechunk native child
buffers; it is not a zero-copy promise. It transfers offsets and validity in
bulk, and leaves the child data native until explicitly extracted. The returned
parts can be passed back to `column_list_nullable` to reconstruct the column.

`column.struct_parts()` returns `StructParts(fields: DataFrame, valid:
list[bool])`. The child dataframe retains field order and types; null parent
rows have null child fields. The mask is necessary to distinguish a null parent
from a present struct whose fields are all null. These parts can be passed to
`column_struct_nullable`. Either method rejects the wrong column dtype.

## Query nested values

| Expression | Behavior |
|---|---|
| `expr.list_len()` | `i64` element count; null list stays null, empty list yields zero |
| `expr.list_get(index)` | Element at a fixed `i64` index; negative indexes count from the end; out of bounds yields null |
| `expr.struct_field(name)` | Named child field; null parent yields null; missing field is an error |

These are native Polars expressions and compose with filtering, projection,
aggregation and other expressions. An extracted element can itself be a list or
struct. Operations validate their input schema during native plan resolution or
collection; creating an expression does not prove that a later input fits it.
Nested casts, list-element callbacks and row-exploding operations are not exposed
by this initial interface.

## Structured schema inspection

Existing `schema()` reports `DType.List` and `DType.Struct` for nested columns.
`DataFrame.schema_tree()` and `Query.schema_tree()` return a preorder
`list[SchemaNode]`, where each node has `parent: i64`, `name: str`, and
`dtype: DType`. Root columns have parent `-1`; other parents index an earlier
node. Struct children retain field names/order; a list has one child named
`item`. This synthetic name cannot collide with a struct field because its
parent identifies its role.

The flat representation avoids recursive `DType` ownership: existing scalar
dtype values stay copyable, printable and comparable. It supports arbitrary
combinations of lists and structs without parsing `DType.Other` descriptions.
Other native dtypes remain `Other` leaves. Schema inspection does not collect
rows, although a file scan can read metadata and fail.

## Parquet compatibility

Lists and structs preserve their semantic types, values and parent/child nulls
through the pinned Polars Parquet reader/writer. Physical encoding and unused
values hidden beneath null parents need not survive. Fixed-size arrays,
arbitrary map semantics and other unexposed dtypes are not promised here.

Polars 0.55.2 has no native Map dtype. Its Parquet schema conversion reads maps
as list/struct data; its Arrow metadata conversion explicitly maps Map to a
large list of key/value structs (`polars-parquet`,
`src/arrow/read/schema/{convert,metadata}.rs`). Use the schema tree and ordinary
list/struct operations for that representation. No dictionary uniqueness,
key ordering, or preservation of the Parquet MAP annotation on write is
promised. This is a supported Polars representation, not a new Plenty Map API.

See the [runnable lesson](../../tutorial/special-purpose/dataframe-nested.md)
and [column types](dataframe-types.md). The existing native allocation-failure
and 32-bit row-count limits also apply to flattened child columns.

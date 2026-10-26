# Lists and records inside columns

A row can contain several values or a record of related fields. Keep these
values in native columns when queries will process them. Nested columns compose
with the same expressions and Parquet I/O as scalar columns.

## Store a list per row

Build one flat child column and supply the boundaries between rows. In this
example the rows contain `[8, 5]`, `[]`, and `[3]`.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    values = df.column_i64("item", [8, 5, 3]?)?
    scores = df.column_list("scores", values, [0, 2, 2, 3]?)?
    report = scores.to_frame()?.lazy()?.select([
        df.col("scores")?.list_len()?.alias("count")?,
        df.col("scores")?.list_get(-1)?.alias("last")?
    ]?)?.collect()?
    print(report.values_i64("count")?)?
    print(report.values_i64_nullable("last")?)?
    Ok(())
```
```output
[2, 0, 1]
[Option[i64].Some(5), Option[i64].Nothing, Option[i64].Some(3)]
```

`list_get` returns null for an out-of-bounds index. Use nullable extraction when
an empty or null list can reach the query. A null child element also remains
null; it is not replaced with zero.

`column_list_nullable` additionally accepts a boolean per row. False marks a
null list; its offsets must describe an empty range. True with an empty range
is an empty list. These states remain distinct through Parquet.

## Group fields into records

A child dataframe supplies the fields of a struct column. Select a field with
`struct_field`; it stays a native expression.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    fields = df.frame([
        df.column_str("name", ["Ada", "Lin"]?)?,
        df.column_i64("score", [8, 5]?)?
    ]?)?
    people = df.column_struct("person", fields)?.to_frame()?
    result = people.lazy()?.select([
        df.col("person")?.struct_field("name")?.alias("name")?
    ]?)?.collect()?
    print(result.values_str("name")?)?
    for node in people.schema_tree()?:
        print(node.parent, node.name, node.dtype)?
    Ok(())
```
```output
["Ada", "Lin"]
-1 person std.dataframe.DType.Struct
0 name std.dataframe.DType.String
0 score std.dataframe.DType.I64
```

The schema tree uses parent indexes: `-1` is a top-level column, and both fields
above belong to node `0`. Lists introduce an `item` child, which may itself be
a struct or another list.

When Plenty needs the values, `frame.column(name)?.list_parts()` returns native
child values plus owned offsets and parent validity. Its child column is named
`item`: use `parts.values.to_frame()?.values_i64_nullable("item")` for nullable
integer elements. `struct_parts()` similarly returns a child dataframe and the
parent mask. Prefer native expressions for filtering or aggregation; extracting
parts is for application code that actually needs them.

Read [the nested-column contracts](../../design/stdlib/dataframe-nested.md) for
ownership, null semantics, reconstruction and Polars' Parquet map representation.

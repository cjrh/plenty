# Dataframe expressions

These operations extend the [native expression plans](dataframe.md#expression-plans).
Every function and method below returns `Result[Expr, DataFrameError]`.
They build immutable Polars expressions, not per-row Plenty callbacks.
Receivers and expression arguments are borrowed; returned plans remain valid
after those inputs are dropped. Invalid column names and incompatible dtypes
can fail during schema resolution or collection.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    data = df.frame([
        df.column_str("name", ["Ada", "lin"]?)?,
        df.column_i64("score", [92, 61]?)?
    ]?)?
    result = df.when(
        df.col("score")?.ge(df.lit_i64(70)?)?,
        df.lit_str("pass")?,
        df.lit_str("retry")?
    )?.alias("result")?
    report = data.lazy()?.with_columns([df.col("name")?.str_upper()?, result]?)?.collect()?
    print(report.values_str("name")?)?
    print(report.values_str("result")?)?
    Ok(())
```
```output
["ADA", "LIN"]
["pass", "retry"]
```

## Literals and conditions

`lit_str(value: &str)` copies a UTF-8 literal into the native plan;
`lit_bool(value: bool)` creates a boolean literal. Alongside `lit(f64)`
and `lit_i64(i64)`, these distinguish literal values from `col(name)`.
`lit_binary(value: &std.bytes.Bytes)` copies an arbitrary byte payload into
the native plan without decoding it as text.

`when(predicate, then_value, otherwise)` accepts three expressions.
For each row, a true boolean predicate selects `then_value`; false or null
selects `otherwise`. Polars determines a common output dtype for the two
branches, so branches can undergo native coercion. An incompatible predicate
or branch type produces a query error.

This is **not short-circuit evaluation**. Both branches must be valid
independently: a condition cannot guard an invalid cast, missing column or
other failing expression. Use an appropriate non-strict cast or validate the
data separately. Nested `when` expressions can represent several choices,
but this API does not translate Plenty `if` statements automatically.

## Explicit casts

`expr.cast(dtype: DType, strict: bool = True)` supports
all numeric `DType` variants (`I8/I16/I32/I64`, `U8/U16/U32/U64`, `F32/F64`),
`Bool`, `String` and `Binary`.
Temporal targets and `DType.Other(...)` fail immediately with `DataFrameError.Schema`;
native dtype descriptions are not executable cast specifications.
Use the explicit [temporal parsing and calendar methods](dataframe-datetime.md)
to construct dates/datetimes in a query.

The default strict mode reports invalid conversions as a query error.
With `strict=False`, failed value conversions become null when Polars
supports that conversion. It does not make every source/target pair legal;
an unsupported conversion can still fail. Existing nulls remain null.

Strict does not mean lossless. For example, casting a finite fractional
float to an integer truncates toward zero, and integer-to-float conversion
can lose precision. Numeric-to-boolean converts zero to false and nonzero
values to true. String parsing and boolean formatting follow Polars, not
Plenty's scalar parsing/formatting rules. Use the typed nullable getters
when a non-strict cast can introduce missing values.
String-to-binary casts preserve the UTF-8 bytes. Binary-to-string casts validate
UTF-8: the pinned engine rejects invalid sequences even with `strict=False`.
Extraction itself never performs these conversions.

## String expressions

| Method | Result dtype | Meaning |
|---|---|---|
| `str_lower()`, `str_upper()` | String | Unicode lowercase or uppercase |
| `str_len()` | I64 | Number of Unicode scalar values |
| `str_contains(pattern: &Expr)` | Bool | Contains a literal substring |
| `str_starts_with(prefix: &Expr)` | Bool | Starts with the literal text |
| `str_ends_with(suffix: &Expr)` | Bool | Ends with the literal text |

Use `df.lit_str("text")?` for a constant pattern, or `df.col("pattern")?`
for row-wise patterns. Null text or a null pattern produces null. A null
predicate is dropped by `filter`. The receiver must be a string expression.
Use string patterns for predictable text semantics: Polars can coerce a
primitive pattern to text, but does not similarly accept a numeric receiver.
This native expression coercion does not change the strict
dtype checks on Plenty's column construction and extraction boundary.

Contains is literal: `"."`, `"*"` and `"["` have no regex meaning.
The engine enables Polars' `regex` crate feature because its literal
contains implementation is behind that feature; no regex API is exposed here.

Character counts are neither UTF-8 byte counts nor grapheme counts.
Combining marks count separately. Casing can change the number of scalars
(for example uppercase `ß` becomes `SS`); no normalization or
locale-specific collation is promised. Embedded NUL bytes remain text.

## Distinct counts and partition windows

`expr.n_unique()` computes an exact distinct count and returns I64.
Unlike `count()`, it counts null as one distinct value. Empty input has
zero distinct values; all-null nonempty input has one. It can be used in
`select`, `group_by` aggregates or a window expression.

`expr.over(partition_by: &list[Expr])` builds a native group-to-rows window.
For example, `df.col("amount")?.sum()?.over([df.col("team")?]?)?`
computes each team's total and broadcasts it back to that team's rows,
without collapsing the frame or converting it into Plenty objects.
Multiple key expressions form composite partitions; null keys group together.

The key list must be nonempty. Empty keys fail immediately with `Schema`;
missing keys and invalid expressions fail when resolving or executing.
Scalar aggregates map to every row in the partition. Non-scalar expressions
must satisfy Polars' group-to-rows length requirements. Output maps back to
the input row positions; it does not introduce a sort order.

This increment has no window ordering, frame bounds, rolling operations,
cumulative sums or ranking methods. Sorting a query does not turn `sum().over`
into a running sum: it still sees the entire partition. Floating reductions
retain the existing native rounding/order caveats.

See [the runnable lesson](../../tutorial/special-purpose/dataframe-expressions.md)
and [exact signatures](signatures.md#stddataframe).

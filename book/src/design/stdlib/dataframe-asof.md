# Integer as-of joins

`Query.join_asof(other, left_on, right_on, strategy=AsOfStrategy.Backward,
tolerance=Nothing, allow_exact_matches=True, suffix="_right")` matches each
left row to at most one nearby right key. It returns a new immutable native
query and borrows both inputs. It remains usable after their owners are dropped.

`left_on` and `right_on` are column names. Both columns must have dtype `I64`;
there is no implicit cast. Derive a named key with `with_columns` when needed.
Floating-point, date/time, grouped and composite as-of keys are not supported.
Use [equality joins](dataframe-joins.md) for exact multi-column matching.

```plenty dataframe
import std.dataframe as df

def main() -> Result[(), df.DataFrameError | AllocError | IoError]:
    trades = df.frame([df.column_i64("second", [3, 7]?)?]?)?
    quotes = df.frame([
        df.column_i64("second", [1, 5, 6]?)?,
        df.column_f64("bid", [9.5, 10.0, 10.5]?)?
    ]?)?
    result = trades.lazy()?.join_asof(quotes.lazy()?, "second", "second")?.collect()?
    print(result.values_f64("bid")?)?
    Ok(())
```
```output
[9.5, 10.5]
```

## Ordering and matching

Both inputs must be sorted ascending by their key, with nulls first. Sorting
is not automatic: use `sort([df.SortKey(df.col("key")?, nulls_last=False)]?)`
when the source is not already ordered.
The native execution checks sortedness and rejects an unsorted input with
`DataFrameError.Query`. The requirement applies to the inputs of the join,
after any preceding filters or key transformations.

| Strategy | Right key chosen |
|---|---|
| `Backward` | Greatest key less than or equal to the left key |
| `Forward` | Smallest key greater than or equal to the left key |
| `Nearest` | Key with the smallest absolute integer distance |

Set `allow_exact_matches=False` to exclude equal keys. `tolerance=Some(n)`
adds an inclusive distance bound of `n` key units; `Nothing` means no bound.
The bound must be nonnegative and fit in `i64`. `Some(0)` permits only exact
matches, so disabling exact matches with zero tolerance yields no matches.
Integer distance comparisons handle the full signed `i64` range without
wrapping at its endpoints.

Backward selects the last right row at the chosen key; forward selects the
first. Nearest breaks equal-distance ties toward the greater key and selects
the last duplicate at its chosen key. These rules refer to right input order:
when duplicate selection matters, explicitly sort by the key and a suitable
secondary column before joining.

Null left keys are unmatched; null right keys are skipped. Null keys never
match one another. Nulls-first placement is validated consistently, including
for inputs whose native metadata already marks them sorted. Every
left row survives once, including duplicates and unmatched rows. An empty
right input supplies null right fields; an empty left input yields no rows.

## Output, failures and costs

Both key columns remain in the output. Right column names that collide with
left names receive the suffix. An unresolved collision is a query error;
columns are not silently overwritten. Unmatched right-side cells are null,
so use nullable extraction unless matching is guaranteed.

Negative tolerances and unsupported key dtypes fail with
`DataFrameError.Schema`. Missing columns, invalid native plans, suffix
collisions and unsorted data are errors rather than unmatched rows. Some
errors arise while the plan is built; data-dependent failures arise when
it is collected or written. Native allocation/panic limits are the same as
for other [dataframe operations](dataframe.md).

The join remains a Polars query-plan node; no Plenty per-row callback or
intermediate Plenty collection is used. A native lazy validation step before
each input scans its entire key column at execution time and returns the frame
unchanged. These steps materialize both inputs and block optimizations from
moving operations across validation. Put intended filters, projections and
sorting before `join_asof` to reduce that work.

The validation steps compensate for a pinned Polars streaming path that can
accept an unsorted right input despite its sortedness-check option. Ordinary
and explicit streaming collection therefore have the same checked contract,
but **as-of joins are not bounded-memory streaming operations**. Plan creation
resolves schemas and does not eagerly collect the datasets.

Rebuild the [optional engine](dataframe-engine.md) to obtain its
`asof_join` feature. Ordinary programs still do not link Polars.

See the [sensor lookup example](../../tutorial/special-purpose/dataframe-asof.md)
and [signature index](signatures.md#stddataframe).

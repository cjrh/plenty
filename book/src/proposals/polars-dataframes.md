# Dataframes backed by Polars

> **Status: Experimental embedding spike.** The optional adapter lives in
> `experiments/polars-embedding/`. It is not a standard-library module and does
> not replace the implemented [`clist`](../design/facilities/01-builtins/clists.md).
> See the [measured evidence](polars-dataframes/evidence.md). Priorities belong
> to [B76 in the backlog](../backlog.md).

## Why use an existing engine?

The original columnar-list vision overlaps with a typed dataframe: ordinary
records describe the data, columns provide efficient storage, and bulk
operations expose parallelism. Polars already supplies a much wider analytical
engine, including expressions, joins, grouping, file readers and query planning.
Its lazy optimizer can push filters and column selection into scans and simplify
expressions. Embedding it lets Plenty focus on a clear, safe language interface
instead of implementing another analytical engine.
[Polars lazy optimizations](https://docs.pola.rs/user-guide/lazy/optimizations/).

## The tested boundary

The spike pins Polars **0.55.2**, disables default features and enables `lazy`
and `parquet`. Their transitive features are substantial; this is not a tiny
dependency. A separately compiled Rust shared library owns `DataFrame`,
`LazyFrame`, expressions and typed column builders. An ordinary Plenty trusted
interface wraps opaque handles in move-only classes with destructors. Existing
foreign-call support is sufficient: there are no compiler or runtime changes.

Plenty creates an expression tree; Polars optimizes and evaluates it. No Plenty
callback runs once per row. For example, the experimental module supports:

```text
groups = [0, 1, 0, 1]?
values = [10.0, 60.0, 80.0, 90.0]?
data = df.from_lists(&groups, &values)?
predicate = df.col("value")?.gt(df.lit(50.0)?)?
weighted = df.col("value")?.mul(df.lit(2.0)?)?.alias("weighted")?
total = df.col("weighted")?.sum()?.alias("total")?
result = data.lazy()?.filter(&predicate)?.with_column(&weighted)?
             .group_by_sum(df.col("group")?, &total)?.collect()?
```

This illustrates the API, not new multiline syntax. The runnable, single-line
chain is in `experiments/polars-embedding/example.plenty`. It produces totals
160 and 300, then repeats the operation through a written and scanned Parquet
file. Polars runs inside the same process, with no Python or subprocess query
execution. Cranelift compiles the Plenty orchestration; Rust/LLVM compiles the
engine separately. Ordinary Plenty compilation does not invoke Rust or Cargo.

## Types and semantics need their own design

The spike has typed `i64` and `f64` input builders and checked typed extraction.
It does **not** implement `DataFrame[T]`: column names and query result schemas
are checked by Polars at runtime. Only a two-column constructor and a small
expression subset are exposed. A production typed facade must distinguish
statically known schemas from schemas discovered in files.

Null extraction becomes `Option[T]`, independently from query failure in
`Result`. Floating-point NaN remains a value; null is not converted to NaN or
zero. Polars defines the query's arithmetic, aggregation, comparison and ordering
semantics. These are not silently promised to match an ordinary Plenty loop:
parallel floating reductions can change rounding, groups have unspecified order,
nulls require explicit policies, and integer overflow needs a deliberate contract.
The executable check observes that Polars keeps NaN in a `value > 50.0` filter,
whereas Plenty's ordinary floating comparison returns false. Translating an
ordinary Plenty predicate into this expression without an explicit semantic
boundary would therefore change the program's result.
The spike only builds floating multiplication and aggregates; it does not claim
checked Plenty integer arithmetic inside a query.
[Polars data types](https://docs.pola.rs/user-guide/concepts/data-types-and-structures/).

Handles retain Polars-owned data after the Plenty source frame dies. Inputs are
borrowed by calls, query operations clone native handles, and errors leave
inputs owned by the caller. Native destruction uses the native allocator.
Recoverable failures currently have coarse error categories. Ordinary Rust
panics are caught at the boundary, but allocation failure may abort the process:
`Result` does not imply Plenty's recoverable `AllocError` contract. Rich error
messages and a documented allocation policy remain important production choices.

## Copies and deployment

Current foreign adapters support scalars and borrowed text, but no numeric slice
borrowing. The spike copies lists element by element into native typed builders,
then materializes Arrow columns. Small results are copied back one scalar at a
time. This is intentionally measured. A bulk numeric-buffer adapter would avoid
per-element calls and intermediate `Option` vectors even without zero-copy.

True zero-copy needs compatible layout, an owner that outlives the array, correct
alignment and null representation, and safe mutation restrictions. Ordinary
Plenty lists use tagged slots; they are not Arrow buffers. Even `clist` columns
cannot simply be handed over without an ownership/release protocol. Parquet
scans avoid the initial Plenty-list conversion altogether.

The engine is optional and prebuilt, with a fixed feature set and its own
deployment artifact. The measured build uses x86-64-v3; this is a chosen spike
hardware floor, not runtime ISA selection or Plenty's normal baseline. A product
must choose supported hardware/targets and package compatible binaries. Statically
adding another independently built Rust runtime archive is not assumed safe;
the shared-library boundary keeps allocation and Rust ownership on their own side.

Keep current `clist` behavior available while evaluating this direction. The
spike tests analytical functionality, not row-mutation compatibility, full-schema
typing, API completeness, or a migration. Its results can support prioritizing
Polars integration without claiming that every small mutable columnar collection
should pay for a query engine.

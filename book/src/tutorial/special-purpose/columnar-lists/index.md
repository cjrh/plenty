# Columnar lists

`clist[T]` makes data-oriented performance accessible through ordinary Plenty
code. Declare a class normally, then choose `clist[YourClass]` when a large
collection of its records is usually processed one or two fields at a time.
You still append ordinary objects and refer to their named fields. The
collection stores each field in its own packed, contiguous column.

For example, averaging temperatures does not need the station identifier from
every reading. Keeping temperatures together lets a scan bring more useful
values into the CPU cache with each memory access. Column pipelines make this
kind of bulk calculation natural: select a field, then use the familiar fluent
operations. The storage choice belongs to this collection; it does not change
how the class is stored everywhere else.

This is useful as a representation as well as a container: it makes efficient
execution easier to express for the programmer and easier to recognize for
the compiler. Start with the access pattern you need, rather than expecting
every operation on a columnar list to become faster. Gathering a complete row
requires reading separate columns. The current implementation accepts only
primitive fields; use ordinary lists for strings, nested objects,
or borrowed references to whole rows.

For analytical expression plans, grouping and Parquet datasets, consider
[dataframes](../dataframes.md). Their optional Polars engine has runtime
schemas and native query semantics; `clist` keeps a lightweight mutable
collection and Plenty's own arithmetic and type checking.

- [Build and scan columnar records](01-scanning.md): reserve capacity, append
  records, scan a column, and update or copy a row.
- [Count matching integer values](02-counting.md): express a filter/count
  calculation that the compiler can accelerate with SIMD.
- [Remove rows and reuse storage](03-removing.md): take individual records or
  clear a batch while retaining capacity.

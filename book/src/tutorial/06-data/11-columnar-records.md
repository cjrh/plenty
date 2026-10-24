# Scan selected fields in columnar storage

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
every operation on a columnar list to become faster.

```plenty
class Reading:
    station: i64
    temperature: f64

def add(total: f64, temperature: f64) -> f64:
    total + temperature

def main() -> Result[(), AllocError | AppendError[Reading] | IoError]:
    mut readings = clist[Reading].with_capacity(3)?
    readings.append(Reading(10, 18.0))?
    readings.append(Reading(20, 21.0))?
    readings.append(Reading(30, 24.0))?
    total = readings.column(Reading.temperature).fold(0.0, add)
    print(total / f64(len(readings)))?
    readings[0].temperature = 19.0
    match readings.copy_row(0):
        case Some(reading):
            print(reading.station, reading.temperature)?
        case Nothing:
            ()
    Ok(())
```
```output
21.0
10 19.0
```

`Reading.temperature` selects that field's column. The pipeline borrows the
collection and yields scalar values, so `add` takes `f64` directly. Reserving
capacity makes the three appends allocation-free after construction. Errors
from append retain the submitted reading; this example propagates that error
so callers can decide whether to retry.

A column pipeline is consumed once. While it remains live, the collection
cannot be changed or moved. Create a new column pipeline for another pass.
`copy_row` gathers a row's fields from the separate columns into an independent
ordinary object. It allocates nothing, but gathering all the fields can cost
more than accessing a record in an ordinary list. Changing the copy does not
change the stored row. Use a column for a selected-field calculation and a row
copy when you actually need the complete object.

The primitive-only restriction keeps this first version's ownership and packed
storage model simple and sound. Prefer an ordinary list for strings, nested
objects, removal, row references, or workloads that usually access whole
objects.

Contiguous columns and explicit scans also give the compiler a useful foundation
for future SIMD and automatic parallel execution. Today these pipelines run
ordinary scalar loops on one thread; choosing `clist` does not enable either
optimization automatically. See the [columnar-list reference](../../design/facilities/01-builtins/clists.md)
for allocation, borrowing, and representation details.

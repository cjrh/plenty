# Build and scan columnar records

Declare a class normally, then create a `clist` to store its fields in separate
columns. This example reserves space for three readings, averages their
temperatures, and updates and copies a row:

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
objects, row references, or workloads that usually access whole
objects.
Indexed field access and row copying compile to direct scalar memory operations,
with bounds checks. This avoids dispatching through a generic runtime helper for
each field, but does not make every access pattern faster. Use column pipelines
for repeated scans when the calculation needs selected fields.

Next, [count matching integer values](02-counting.md) with an automatically
optimized column pipeline. See the
[columnar-list reference](../../../design/facilities/01-builtins/clists.md)
for the allocation, borrowing and representation contracts.

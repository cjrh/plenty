# Remove rows and reuse storage

Use `remove(index)` when you need to take one complete record out of a columnar
list. It returns `Some(record)` or `Nothing` for an out-of-bounds index. Remaining
records keep their order, and negative indices count from the end.

```plenty
class Reading:
    station: i64
    temperature: f64

def main() -> Result[(), AllocError | AppendError[Reading] | IoError]:
    mut readings = clist[Reading].with_capacity(3)?
    readings.append(Reading(10, 18.0))?
    readings.append(Reading(20, 21.0))?
    readings.append(Reading(30, 24.0))?
    match readings.remove(1):
        case Some(reading):
            print("removed", reading.station, reading.temperature)?
        case Nothing:
            ()
    for station in readings.column(Reading.station):
        print(station)?

    capacity = readings.capacity()
    readings.clear()
    print(len(readings), readings.capacity() == capacity)?
    readings.append(Reading(40, 19.0))?
    print(readings[0].station)?
    Ok(())
```
```output
removed 20 21.0
10
30
0 True
40
```

Removal assembles an independent ordinary object, just like `copy_row`, then
shifts the later values left in every column. It allocates nothing, but removing
near the front of a large collection moves much more data than removing the
last row. Repeated front removals are a poor way to filter a whole dataset.

`clear()` empties all columns in constant time and retains their allocation.
This is useful when processing successive batches: clear the old batch and
append the next one into the same storage. Removal also retains capacity.
Appends within that capacity do not allocate, although their return type still
requires error handling. Use `drop(readings)` when you want to release the
storage instead of reusing it.

Both methods require mutable access. A live column pipeline borrows the entire
collection, so finish its traversal before removing rows or clearing the owner.
See the [reference](../../../design/facilities/01-builtins/clists.md) for the
complete contract and costs.

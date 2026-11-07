# Summarizing a large stream

Use `std.sketch` when keeping every value costs too much memory and an approximate
answer is useful. Keep exact data when correctness requires exact membership or
counts. A sketch's memory budget and accuracy are part of the choice.

This example counts distinct numeric customer identities, samples latency
quantiles, and tracks frequent customer identities without retaining the stream:

```plenty
import std.sketch as sketch
from std.sketch import SketchError
from std.bytes import BytesError

def main() -> Result[(), AllocError | IoError | SketchError | BytesError]:
    mut customers = sketch.hyperloglog(10)?
    mut latency = sketch.quantiles(128, seed=42u64)?
    mut frequent = sketch.frequent_items(16)?
    for index in range(1000):
        customer = u64(index % 100)
        customers.add(customer)
        latency.add(f64(index % 50))?
        frequent.add(customer)?
    print(latency.count(), latency.retained())?
    print(latency.quantile(0.0)?, latency.quantile(1.0)?)?
    print(customers.estimate() > 80.0 and customers.estimate() < 120.0)?
    Ok(())
```
```output
1000 128
0.0 49.0
True
```

The quantile sketch retains a uniform sample and exact extrema. Its interior
quantiles are sample estimates, not guaranteed rank bounds. Frequent-item queries
return lower/upper count bounds, and `candidates()` returns possible heavy
hitters. Increase capacity for better accuracy. See the
[reference](../../design/stdlib/sketch.md) for the different guarantees.

## Skipping unnecessary lookups

A static xor filter can cheaply rule out keys before an expensive exact lookup.
A positive answer still needs that lookup:

```plenty
import std.sketch.xor as xor
from std.sketch import SketchError
from std.bytes import BytesError

def main() -> Result[(), AllocError | IoError | SketchError | BytesError]:
    keys = [10u64, 20u64, 30u64]?
    filter = xor.build(&keys, max_keys=100)?
    print(filter.len(), filter.contains(20u64))?
    Ok(())
```
```output
3 True
```

For a changing set, `std.sketch.cuckoo` supports bounded growth and deletion.
Insertion returns a layer receipt. Keep it with the real record; deletion must
use that receipt and a known live occurrence, never just a positive membership
test:

```plenty
import std.sketch.cuckoo as cuckoo
from std.sketch import SketchError
from std.bytes import BytesError

def main() -> Result[(), AllocError | IoError | SketchError | BytesError]:
    mut filter = cuckoo.cuckoo(initial_buckets=16, max_buckets=112)?
    layer = filter.insert(42u64)?
    print(filter.contains(42u64))?
    print(filter.remove(42u64, layer)?, filter.len())?
    Ok(())
```
```output
True
True 0
```

All three sketches and both filters provide versioned `to_bytes()` checkpoints
and bounded readers. Save the application's key-hashing convention and cuckoo
receipts too. See [xor filters](../../design/stdlib/sketch-xor.md) and
[cuckoo filters](../../design/stdlib/sketch-cuckoo.md) before choosing one.

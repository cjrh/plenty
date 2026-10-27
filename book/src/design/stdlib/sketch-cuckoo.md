# `std.sketch.cuckoo`

`cuckoo(initial_buckets=64, max_buckets=1048576, seed=0)` creates a scalable
approximate-membership filter for u64 keys. Initial buckets must be a power of
two between 1 and 1,048,576. The total bucket limit is at least that large and
at most 16,777,216. Each bucket has four two-byte fingerprints. Layers double
geometrically; the caller's limit counts buckets across **all** layers.

`insert(key)` returns the **layer index** receiving that occurrence. Every call
inserts an occurrence, including duplicates. `contains(key)` searches all
layers. `len()` counts live inserted occurrences, not distinct keys;
`layers()` and `buckets()` expose current storage growth.

An existing layer is tried with at most 256 eviction steps. A failed eviction
chain is rolled back completely before trying another layer or allocating a
new one. Allocation failure or `SketchError.Limit` preserves the filter and its
membership. Construction allocates bounded eviction scratch; insertions into
existing layers and membership/deletion queries allocate nothing.

Keep the layer index that `insert` returns; `remove` needs it:

```plenty
from std.bytes import BytesError
from std.sketch import SketchError
from std.sketch.cuckoo import cuckoo

def main() -> Result[(), SketchError | AllocError | BytesError | IoError]:
    mut seen = cuckoo()?
    receipt = seen.insert(42u64)?
    seen.insert(7u64)?
    print(seen.len(), seen.contains(42u64))?
    print(seen.remove(42u64, receipt)?, seen.len(), seen.contains(7u64))?
    Ok(())
```
```output
2 True
True 1 True
```

## Deletion requires the insertion receipt

Keep the index returned by insertion alongside the authoritative record.
`remove(key, layer_index)` requires a **known live occurrence inserted into that
layer of this filter**. It returns `Result[bool, SketchError]`; an invalid layer
index is `InvalidArgument`. The layer indices survive serialization and remain
stable during growth. Empty layers are retained.

A positive `contains` is insufficient evidence for deletion: it may be a false
positive. Also, a key may falsely match an earlier layer even when it really
exists in a later layer. Supplying the receipt avoids that cross-layer ambiguity.
Removing an absent occurrence, reusing a receipt, or using the wrong filter/layer
can erase another key's fingerprint and cause false negatives. Do not deduplicate
inserts using `contains`; doing so breaks deletion accounting. Applications that
cannot retain this information should use the filter only for membership checks.

Within a layer, equal fingerprints sharing a bucket share both candidate buckets,
so deleting one of the corresponding known occurrences remains sound. The
fingerprints do not identify which colliding occurrence was removed.

## Accuracy, cost and persistence

Intact filters have no false negatives under the deletion precondition. False
positives are approximately 8/65536 per full layer for ordinary hashes, increasing
with the number of layers. Zero fingerprints map to one, causing a small bias.
This is not an adversarial-key or security guarantee. Queries cost O(layers),
with up to eight slot comparisons per layer. Growth initializes a new array and
may be slower than a successful insertion into an existing layer.

`to_bytes()` writes `PSC1`, seed, total configured limit, initial bucket count,
layer count, and occurrence count as little-endian u64, followed by little-endian
u16 slots in layer order. `from_bytes(data, max_buckets)` accepts a `ByteSource`,
validates shape, counts, length and the caller limit before constructing storage,
and returns `SketchError | AllocError | BytesError`. The caller limit bounds the serialized
**configured maximum**, not only today's occupancy. Serialization exposes
`BytesError`. The format is versioned, Plenty-specific and unauthenticated.

## Acknowledgements

- Uses the cuckoo filter from ["Cuckoo Filter: Practically Better Than Bloom"](https://doi.org/10.1145/2674005.2674994) by Bin Fan, David G. Andersen, Michael Kaminsky and Michael D. Mitzenmacher (2014).
- Layered growth takes the Rust [`scalable_cuckoo_filter`](https://crates.io/crates/scalable_cuckoo_filter) crate as a design reference; no code is taken from it.

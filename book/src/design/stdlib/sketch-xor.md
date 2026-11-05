# `std.sketch.xor`

A static approximate-membership filter over u64 keys. `build(keys, max_keys:
NonNegative[i64], seed=0)` accepts duplicates, sorts/deduplicates a fallible copy, and returns an
`XorFilter`. The input is not mutated. `max_keys` bounds **input entries**, not
just distinct keys; a `max_keys` above 16,777,216 traps.

```plenty
from std.sketch import SketchError
import std.sketch.xor as xor

def main() -> Result[(), SketchError | AllocError | IoError]:
    keys = [10u64, 20u64, 30u64, 20u64]?
    filter = xor.build(&keys, 1000)?
    print(filter.len(), filter.contains(20u64), filter.contains(25u64))?
    Ok(())
```
```output
3 True False
```

`contains(key)` allocates nothing and reads three fingerprint positions. Inserted
keys have no false negatives for an intact filter. Absent keys may return true:
approximately 1/256 with well-distributed hashes. This is not an exact set,
security boundary, or an adversarial-key guarantee. `len()` counts distinct
input keys; the empty filter always returns false.

Construction uses three disjoint blocks, graph peeling, and reverse XOR8
fingerprint assignment. The persistent filter has `3 * (1 + unique_count // 2)`
one-byte slots, plus metadata. At the maximum key count, each block has
8,388,609 slots. Position reduction multiplies a 32-bit hash fragment by this
bounded block size without overflow; only the SplitMix64 mixer and attempt-seed
schedule use explicit modular arithmetic. Scratch memory is O(input entries + slots),
considerably larger than the final filter. Sorting is O(n log n), followed by
up to 32 O(n) peeling attempts. Failure to peel returns
`SketchError.ConstructionFailed`; allocation failures return
`SketchError.Allocation(cause)`.
See [Graf and Lemire's xor-filter paper](https://arxiv.org/abs/1912.08258).

This first implementation deliberately uses more construction slack than a
tuned xor/fuse filter. It is immutable after construction. For changing data,
see [cuckoo filters](sketch-cuckoo.md).

`to_bytes()` returns `Result[Bytes, AllocError]`. `from_bytes(data,
max_keys: NonNegative[i64])` reads a `&[u8]` view and returns `SketchError`.
The `PSX1` format stores seed, unique count, block size (little-endian u64), then
fingerprint bytes. Readers validate the count/shape and exact length before
allocation, and reject values beyond the caller's limit. It is a Plenty-specific
versioned checkpoint, not the `xorf` crate's representation. It has no integrity
checksum: valid-looking corruption can introduce false negatives.

## Acknowledgements

- Uses the [xor filter](https://arxiv.org/abs/1912.08258) by Thomas Mueller Graf and Daniel Lemire (2020).

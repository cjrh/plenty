# `std.sketch.xor`

A static approximate-membership filter over u64 keys. `build(keys, max_keys,
seed=0)` accepts duplicates, sorts/deduplicates a fallible copy, and returns an
`XorFilter`. The input is not mutated. `max_keys` bounds **input entries**, not
just distinct keys, and must be between 0 and 16,777,216.

`contains(key)` allocates nothing and reads three fingerprint positions. Inserted
keys have no false negatives for an intact filter. Absent keys may return true:
approximately 1/256 with well-distributed hashes. This is not an exact set,
security boundary, or an adversarial-key guarantee. `len()` counts distinct
input keys; the empty filter always returns false.

Construction uses three disjoint blocks, graph peeling, and reverse XOR8
fingerprint assignment. The persistent filter has `3 * (1 + unique_count // 2)`
one-byte slots, plus metadata. Scratch memory is O(input entries + slots),
considerably larger than the final filter. Sorting is O(n log n), followed by
up to 32 O(n) peeling attempts. Failure to peel returns
`SketchError.ConstructionFailed`; allocation failures return `AllocError` or
`BytesError.Allocation` for the packed fingerprints.
See [Graf and Lemire's xor-filter paper](https://arxiv.org/abs/1912.08258).

This first implementation deliberately uses more construction slack than a
tuned xor/fuse filter. It is immutable after construction. For changing data,
see [cuckoo filters](sketch-cuckoo.md).

`to_bytes()` returns `Bytes` with `BytesError` on failure. `from_bytes(data,
max_keys)` accepts any `ByteSource` and returns `SketchError | BytesError`.
The `PSX1` format stores seed, unique count, block size (little-endian u64), then
fingerprint bytes. Readers validate the count/shape and exact length before
allocation, and reject values beyond the caller's limit. It is a Plenty-specific
versioned checkpoint, not the `xorf` crate's representation. It has no integrity
checksum: valid-looking corruption can introduce false negatives.

## Acknowledgements

- Uses the [xor filter](https://arxiv.org/abs/1912.08258) by Thomas Mueller Graf and Daniel Lemire (2020).

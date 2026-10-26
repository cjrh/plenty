# `std.sketch`

Bounded summaries for streams too large to retain. These pure-Plenty algorithms
own their memory and random state. They do not wrap Apache DataSketches or share
its serialized formats. Also see [xor filters](sketch-xor.md) and
[cuckoo filters](sketch-cuckoo.md).

## Distinct identities

`hyperloglog(precision=12, seed=0)` creates a dense `HyperLogLog`. Precision is
4–18; storage is exactly `2**precision` packed one-byte registers plus `Bytes` metadata.
`add(key: u64)` and `merge(other)` allocate nothing. Duplicate keys are harmless.
Merge requires equal precision and seed and leaves the receiver unchanged on
`SketchError.Incompatible`. Estimate and merge cost O(registers).

`estimate()` uses the original HyperLogLog harmonic estimate with linear counting
for small cardinalities. `relative_standard_error()` returns `1.04/sqrt(m)`:
an asymptotic standard error, **not** a guaranteed error bound. There is no HLL++
bias table or near-64-bit-universe correction. Use this for ordinary cardinality
estimation, not exact counts or estimates approaching the hash-space size.
See the [HyperLogLog paper](https://www.chrismusco.com/amlds2020/resources/hyperloglog.pdf).

Keys are numeric identities. `hash_u64(key, seed=0)` is a stable SplitMix64 mixer,
not a cryptographic hash. The algorithms are designed for ordinary data, not
adversarially chosen keys. Applications mapping text or compound values to u64
must choose and persist their own stable hash convention; collisions in that
mapping cannot be undone by a sketch.

## Quantiles

`quantiles(capacity=1024, seed=0)` creates `Quantiles`, a uniform sample without
replacement using [reservoir Algorithm R](https://www.cs.usfca.edu/~mmalensek/cs677/schedule/papers/vitter1985random.pdf).
Capacity is 1–1,048,576 finite f64 values. `add(value)` rejects NaN/infinities;
`count()` is the number of accepted values and `retained()` is at most capacity.
Construction reserves the sample buffer; updates allocate nothing, maintain it
in sorted order, and cost O(capacity) when replacing a sample. Queries cost O(1).

`quantile(fraction)` accepts a finite number in [0,1]. Empty streams return
`SketchError.Empty`. Endpoints return the exact minimum/maximum. Other fractions
use the nearest-rank order statistic of the retained sample (no interpolation).
Results are exact before sampling starts. Afterwards they are probabilistic;
there is no deterministic rank-error guarantee or tail-relative guarantee.
Increase capacity to improve accuracy. This first API has no quantile merge.
It is a simple reservoir, not KLL, REQ, or a t-digest.

The sketch owns a `std.random.Rng`. The seed, accepted input order, and capacity
determine its sample. Serializing and restoring preserves continued sampling.
The random generator's existing version-stability contract is unchanged.

## Frequent identities

`frequent_items(capacity=128)` creates a `FrequentItems` Misra–Gries summary with
1–65,536 counters. It accepts unit-weight u64 identities; there is no weighted
update or merge in this API. Construction allocates two fixed arrays.
`add(key)` and `frequency(key)` allocate nothing and cost O(capacity).

`frequency(key)` returns `Frequency(key, lower, upper)` with deterministic bounds
on the true count. `error_bound()` is the number of full counter decrements,
at most `count() // (capacity + 1)`. A missing candidate has lower bound zero
and upper bound `error_bound()`. `candidates()` allocates a list of retained
nonzero counters in storage order, not frequency order. Every key whose true
frequency exceeds `error_bound()` appears in that list. A second pass over the
original data can compute exact counts for these candidates.

## Errors and persistence

`InvalidArgument` covers invalid configuration or nonfinite input. Count overflow
returns `Limit` without accepting the update. List-backed constructors expose
`AllocError`; packed byte storage exposes `BytesError.Allocation(AllocError)`.
The generated signature index lists each precise union. Methods that serialize
to `std.bytes.Bytes` return `BytesError`. All allocation failures remain recoverable.

`to_bytes()` writes a versioned Plenty format: `PSH1`, `PSQ1`, or `PSF1`.
Integers use little-endian u64 words. Quantile values use 32-byte, space-padded,
shortest round-trip scientific ASCII slots; this preserves finite f64 values,
including signed zero, without host-endian assumptions. These are checkpoint
formats, not interchange formats with third-party sketch libraries.

`hyperloglog_from_bytes(data, max_precision=18)`,
`quantiles_from_bytes(data, max_capacity=1048576)`, and
`frequent_items_from_bytes(data, max_capacity=65536)` reject unknown versions,
bad lengths, invalid state and trailing bytes. Pass a smaller caller limit when
reading untrusted checkpoints. HLL and frequent-item readers accept a
`ByteSource`; the quantile reader takes `Bytes` for validated float parsing.
Formats are version-tagged: future incompatible formats need a new version and
must not silently reinterpret these bytes. They contain no checksum or
authentication; structurally valid corruption can change estimates.

The crate audit for issues #496–#498 found infallible allocations in
`datasketches` 0.5.0 (`hll/array8.rs`, `req/compactor.rs`), `xorf` 0.13.0
(`prelude/mod.rs`), and `scalable_cuckoo_filter` 0.5.1 (`bits.rs`). Small direct
implementations keep Plenty's recoverable allocation policy without a native
dependency or ABI.

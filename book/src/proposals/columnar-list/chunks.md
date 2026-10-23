# Chunks, deletion, and statistics

These are extensions considered for [columnar lists](../columnar-list.md).
The initial candidate stays dense and flat. Keeping these choices separate lets
early measurements answer whether the basic storage interface is worthwhile.

## Fixed-size chunks

A chunk contains the same bounded number of rows in every field column. A
1,024-row chunk of eight 64-bit fields holds 64 KiB of payload; a 4,096-row chunk
holds 256 KiB. Those are experiment sizes, not chosen public defaults. A scan
walks the selected column of each chunk and then advances to the next chunk.

Chunking can bound relocation, permit incremental allocation and release whole
empty chunks. It also adds a directory, allocation count, tails, address
calculations and transitions between buffers. A single aligned chunk allocation
with generated column offsets is an alternative to one allocation per field;
the spike uses the latter and does not settle the former. Tune byte footprint
as well as row count, since class width varies.

[Parquet's row-group model](https://parquet.apache.org/docs/concepts/) is a useful
analogy for grouping rows across columns, but its pages are encoded file data.
In-memory chunk size should follow workload/cache measurements, not Parquet's
disk-oriented recommendations. The measured flat SoA scans beat both tested
chunk sizes; chunking needs a growth, mutation or pruning benefit to earn its cost.

## Deletion is an identity decision first

| Policy | Benefit | Cost or observable change |
| --- | --- | --- |
| Ordered dense removal | Familiar list positions and iteration | Shift later values in every field |
| Swap with last row | Constant-sized transfer | Changes order and the moved row's position |
| Tombstones | Avoid immediate relocation | Holes complicate indexing, scans and storage reclamation |
| Generational handles | Identity can survive relocation | Mapping storage, stale-handle checks and generation exhaustion |

Do not give `clist[i]` both dense-list and physical-slot meanings. A tombstoned
design needs either an explicit sparse-slot/handle API or rank/select metadata
to map logical live indices to storage. The latter has costs even before vacuum.
Do not reuse a tombstone for a new appended row if iteration promises insertion
order, unless an ordering structure preserves that promise. A bitmap branch can
also inhibit vectorization; processing dense live runs or vector masks is a
later optimization.

If tombstones are adopted, deletion should normally destroy owned payloads at
deletion time, clear liveness and decrement the live count. Vacuum must move
only live owners, never read destroyed slots and never run their destructors
again. Keeping resources alive until vacuum would be a different, visible
ownership contract, not a storage detail.

An explicit `vacuum()` is preferable to an unexplained latency spike. Specify
whether it compacts within chunks, across chunks, or also releases spare
capacity. In-place stable compaction need not allocate, but it may not return
buffer capacity to the allocator. Repacking into fewer allocations can fail;
reserve destinations first and retain a valid container on failure. Outstanding
loans prohibit either operation. A slot-based API must say that vacuum
invalidates positions, or use handles whose mapping is updated. A default
threshold cannot substitute for that identity contract.

The ownership experiment exhausts all 256 deletion masks of eight rows,
including repeated deletion and repeated vacuum, with counted affine owners.
It validates immediate destruction and stable compaction, not a production
bitmap, chunk directory, relocation handle scheme, or deletion-speed claim.

## Statistics need a validity contract

[DuckDB's zonemap guidance](https://www.duckdb.org/docs/current/guides/performance/indexing)
explains why per-group bounds help clustered data much more than shuffled data.
[Parquet page indexes](https://parquet.apache.org/docs/file-format/pageindex/)
similarly separate bounds metadata from payload reads to skip irrelevant pages.
For clist, start with optional conservative integer bounds for explicit
field comparisons, only after chunking itself is justified.

Deleting rows may leave old min/max values as a wider conservative bound:
that loses pruning opportunities but does not hide a matching live row.
Appending or mutating a value outside the bound must widen it or mark the
statistics invalid. A mutable field loan must mark the affected stats dirty
*before* exposing the address; recompute on demand or scan without pruning.
NaNs, signed zero, nullable values, string ordering and unknown comparisons
need explicit rules; "invalid/unknown means scan" is the safe fallback.

Exact sums and counts have stronger maintenance requirements. Cached sums
cannot silently change checked-overflow traps or floating-point reduction order.
Nor may a filter skip an arbitrary callback that prints, mutates a capture,
allocates or traps. Recognize a bounded effect-free field comparison or expose
an explicit typed query API; otherwise run the ordinary pipeline. This is the
same semantic restraint required by [automatic SIMD](../std-simd/automatic.md).

The spike's min-bound test finds one candidate group out of eight for sorted
integers and all eight for a permutation of the same values, with identical
results. It does not time metadata construction or claim a general query speedup.
Compression, secondary indexes, joins, transactional updates and automatic
query planning remain outside this container proposal.

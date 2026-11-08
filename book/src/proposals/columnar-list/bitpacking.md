# Bitpacked clist columns

Issue #655 asked whether block bitpacking would speed up scans of large integer
`clist` columns with narrow value ranges. The investigation compared it with
the native columns and the [automatic SIMD counts](simd-counts.md) that exist
today, and measured the mutation and memory costs a packed representation
would add.

**Result:** bitpacking is not worth adding to `clist`, as its storage or as a
separate read-only representation. On the SSE2 baseline the decode costs about
as much per row as the native vector comparison it replaces. A narrower native
field type gives a larger speedup and keeps every mutation contract. The
automatic counts now cover 8- and 16-bit columns (issue #904). Large wins on sorted or clustered data come from skipping blocks
by their min/max, which needs no bitpacking and belongs with
[chunk statistics](chunks.md).

## Method

The kernels are in C, so they show the best a packed layout could achieve
before any Plenty lowering costs. Each one counts `v > t` over one `u32` column:

| Kernel | Storage | Notes |
|---|---|---|
| native scalar | `u32` | Plenty's scalar path |
| native SSE2 | `u32` | 4 lanes, the shape of Plenty's SIMD count |
| native u16/u8 SSE2 | `u16`/`u8` | 8 and 16 lanes, the values cast to the narrow type |
| native zone map | `u32` plus min/max per 128 rows | Skips blocks the predicate settles |
| FOR, decode all | bitpacked | Rebuilds and compares every value |
| FOR, skip | bitpacked | Settles blocks from min and width, otherwise compares residuals |
| delta, decode all/skip | bitpacked deltas | Sorted data only |

The bitpacked layout is simdcomp's 4x layout, as used by the Rust `bitpacking`
crate's `BitPacker4x`. Each 128-value block has its own bit width and
frame-of-reference minimum. Value `i` of a block lives in lane `i % 4`, so one
vector shift and mask yields four values. Each width has a fully unrolled
decoder. Delta blocks store `v[i] - v[i-4]` and a base vector, so decoding is
one vector add per four values and each block decodes on its own. Every encoding
is checked by a round trip, and every scan result is checked against the scalar
count.

The data are 16,384 rows (cache-resident), 1,048,576 rows (4 MiB, larger than
L2) and 67,108,864 rows (256 MiB, larger than L3). The distributions are:

- uniform values of 4, 8, 12, 16 and 24 bits;
- 8-bit values with 1% of rows up to 20 bits ("outliers");
- sorted values with 4-bit increments.

Each case uses thresholds matched by 99%, 50% and 1% of rows. The C is compiled
with gcc 16.2.1 at `-O2 -fno-tree-vectorize` and uses SSE2 intrinsics only.
Plenty's own paths are counted from release builds of `scan.plenty`.

## Scan cost

Instructions per row come from `perf_event_open` around the measured scans.
They do not depend on load and are identical across runs:

| Data (1,048,576 rows, 50% match) | native scalar | native SSE2 | u16 SSE2 | u8 SSE2 | FOR decode all | FOR skip |
|---|---:|---:|---:|---:|---:|---:|
| uniform 4-bit | 5.00 | 2.25 | 1.12 | 0.57 | 2.11 | 2.01 |
| uniform 8-bit | 5.00 | 2.25 | 1.12 | 0.57 | 2.03 | 1.90 |
| uniform 12-bit | 5.00 | 2.25 | 1.12 | | 2.49 | 2.31 |
| uniform 16-bit | 5.00 | 2.25 | 1.12 | | 2.05 | 1.87 |
| uniform 24-bit | 5.00 | 2.25 | | | 2.59 | 2.50 |
| outliers 8-bit | 5.00 | 2.25 | | | 2.56 | 2.41 |
| sorted, 4-bit steps | 5.00 | 2.25 | | | 2.43 | 0.11 |

Delta decoding costs 2.64 instructions per row on the sorted data, or 0.06 with
skipping. Plenty's current paths cost 3.0 instructions per row for the `i32`
SIMD count, and 13.5 and 12.5 for the same pipeline over `u16` and `u8` columns,
which run scalar.

SSE2 has only two-operand instructions, so each extracted lane group costs a
register copy as well as its shift, mask, compare and accumulate. gcc's fully
unrolled decoder also spills. Compiling the same intrinsics for x86-64-v3
(three-operand VEX encoding) lowers native SSE2 to 1.75 and FOR to 1.48–2.25
instructions per row. The ranking does not change.

Elapsed time, from the first full run pinned to CPUs 0,2,…,14, in ns/row, best
of five trials. The host load average was 8–11 from other agents' builds, so
treat these as ranges:

| Data, 50% match | Rows | native SSE2 | u16 SSE2 | u8 SSE2 | FOR decode all | FOR skip |
|---|---:|---:|---:|---:|---:|---:|
| uniform 8-bit | 16,384 | 0.112 | 0.059 | 0.031 | 0.151 | 0.115 |
| uniform 8-bit | 1,048,576 | 0.091 | 0.042 | 0.022 | 0.106 | 0.094 |
| uniform 8-bit | 67,108,864 | 0.117 | 0.055 | 0.028 | 0.099 | 0.086 |
| uniform 16-bit | 67,108,864 | 0.117 | 0.056 | | 0.115 | 0.096 |
| uniform 24-bit | 67,108,864 | 0.121 | | | 0.146 | 0.129 |
| outliers 8-bit | 67,108,864 | 0.119 | | | 0.172 | 0.151 |

This run is retained as `first-run.jsonl`. The runner's `results.json` comes
from a second run under a load average of 14–37; it is slower throughout, with
the same ranking and identical instruction counts.

- **In cache**, bitpacked scans are no faster than native SSE2 and often
  slower. The decode work replaces the load work.
- **Larger than cache**, the native `u32` scan reads about 34 GB/s. The 8-bit
  FOR column reads 3.7 times fewer bytes but scans only 1.2–1.4 times faster,
  because it is limited by instruction throughput, not by memory. Wider or
  irregular data (24-bit, outliers) is slower than native.
- **Narrow native columns** scan 2 times (`u16`) and 4 times (`u8`) faster
  than native `u32` at every size, and 3 times faster than 8-bit FOR.
- **Selectivity** does not change branchless counts. It matters only when
  whole blocks can be settled.

## Block skipping

On sorted data, per-block min/max settles almost every block. The zone-map scan
over a native column costs 0.07–0.10 instructions per row, against 2.25 for
native SSE2. FOR and delta skipping reach the same 0.06–0.17 from the metadata
they already store. This gain comes from block statistics, not packing: the
native zone map has the same benefit at 4.06 bytes per row.

On uniform data, few blocks can be settled, and the zone-map scan is slower
than plain SIMD even though it executes fewer instructions (1.78–2.16 against
2.25 per row at 1% selectivity on 1M and 64M rows). The slowdown grows with
size: about 1.5 times at 1M rows and 0.37–0.43 against 0.12–0.13 ns/row (about 3 times) at 64M rows.
A review measured 0.42 branch misses per block, accounting for about 7% of the
extra cycles at 64M rows, so the skip branch is not the main cost. The cause
was not measured; memory access resuming after skipped blocks is a plausible
one. The kernel's per-block call also adds about 0.5 instructions per row.
Statistics
would need explicit opt-in or an adaptive fallback, and invalidation on writes,
as the [chunk statistics proposal](chunks.md) already discusses.

## Memory

Including the width, minimum and block offset (9 bytes per 128 rows, 0.07 bytes
per row):

| Data | native `u32` | FOR | delta | narrow native |
|---|---:|---:|---:|---:|
| uniform 4-bit | 4.00 | 0.57 | | 1.00 (`u8`) |
| uniform 8-bit | 4.00 | 1.07 | | 1.00 (`u8`) |
| uniform 16-bit | 4.00 | 2.07 | | 2.00 (`u16`) |
| uniform 24-bit | 4.00 | 3.07 | | |
| outliers 8-bit | 4.00 | 2.10 | | |
| sorted, 4-bit steps | 4.00 | 1.33 | 0.98 | |

Per-block widths make outliers expensive. With 1% wide values, about 72% of
128-row blocks contain one, and the column doubles. Below 8 bits, bitpacking
saves memory that a native byte column cannot.

## Mutation

These are the operations a mutable packed `clist` would have to support, on
1,048,576 uniform 8-bit rows. Per-access counts include the benchmark loop:

| Operation | native | FOR |
|---|---:|---:|
| Indexed read (random) | 6 instructions | 40 instructions |
| Indexed write that fits the block | 6 instructions | 54 instructions |
| Write that widens its block | 6 instructions | ~5,400 instructions to repack 128 rows, plus resizing the block |
| Ordered removal, per following row | 0.2 instructions (memmove) | 39 instructions (decode, shift, re-encode); ~50 times slower |
| Append, per row | one store | ~35 instructions to encode each completed block |
| Decode to a native array | | 3.8 instructions/row |

A widening write is the decisive case. Changing a block's width changes its
size. A contiguous layout must move every later block, which makes
`items[i].field = value` take O(n) time. Separately allocated blocks avoid that,
but make a field write allocate and possibly fail. Today that write is checked,
infallible and allocation-free. Reserving the widest block size avoids both
and loses all the compression. Ordered removal changes every following block,
because rows move across block boundaries.

## Recommendations

- **Do not bitpack `clist` storage.** The scan gain is small (at most 1.4 times
  on columns larger than cache, none in cache). It would break the write and
  allocation contracts, and slow reads, appends and removals.
- **No separate read-only packed representation now.** It would need its own
  type, construction and freezing API, scan lowering, and vector shifts that
  `std.simd` lacks. All this to beat native SIMD by at most 1.4 times, on data
  that does not fit a narrower type. Reconsider only with a measured workload of
  columns larger than cache with irregular ranges, together with wider vectors
  and runtime ISA selection ([B23](../../backlog.md)).
- **Extend the automatic SIMD counts to 8- and 16-bit integer columns**
  (issue #904, done). Choosing the narrowest field type already reduces memory as
  much as 8- or 16-bit bitpacking; the vector count would make those columns
  the fastest to scan rather than the slowest.
- **Block statistics, not packing, are the route to skipping.** Evaluate them
  with [chunks and statistics](chunks.md), where the invalidation costs and
  the shuffled-data slowdown measured here apply.
- **No public codec or Rust format compatibility.** No `std` module or
  interchange format needs bitpacked integers. Wrapping the Rust crate would
  bring a format without a consumer; a codec written in Plenty would also need
  vector shifts first.

## Limitations

The kernels are C on one x86-64 machine (Ryzen 7 7840HS), single-threaded, in
hot repeated scans. All kernels share one compiler and harness, so they compare
fairly with each other; Plenty's own vector count costs 3.0 instructions per row
against 2.25 for the matching C kernel. The C kernel compares unsigned values
and pays a bias `xor` that Plenty's signed `i32` count does not, so Plenty's
extra cost is somewhat more than 0.75 instructions per row. Only
128-row blocks and the 4x layout were measured. Wider-vector layouts (8x with
AVX2) were not measured, since Plenty targets the SSE2 baseline. Timings were
taken while other agents were building. The instruction counts, and the
ranking across both runs, are the evidence.

## Reproduction

The experiment in `experiments/clist-bitpacking/` builds the kernels, counts
Plenty's own paths and writes all measurements:

```sh
cargo build --locked --release --bin plenty
python3 -B experiments/clist-bitpacking/run.py target/release/plenty --out target/bitpack
```

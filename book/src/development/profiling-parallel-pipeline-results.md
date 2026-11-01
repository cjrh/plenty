# Parallel pipeline measurements

Historical measurements; see [Profiling the compiler](profiling.md) for the profiling guide.

## Parallel pipelines

`scripts/bench_par_pipelines.py` compares a serial pipeline, the equivalent
`pool.map`/`reduce_tree` calls, and the same pipeline after `.par(&pool)` (B75
stage 4). Cases are `map`+`collect`, `filter`+`count`, `sum` over `i64` lists,
`i64` ranges and `f64` lists, and `reduce`. The cheap callback is `x * 3 + 1`;
the expensive one runs 100 rounds of a linear congruential step. The `pool`
column for `filter`+`count` maps the predicate to `0`/`1` and sums serially;
for the sums and `reduce` it is `reduce_tree` over `pool.map`. Run:

```sh
taskset -c 0,2,4,6,8,10,12,14 python3 -B scripts/bench_par_pipelines.py target/release/plenty
```

The `taskset` mask keeps the eight-worker pool on the eight physical cores of
this laptop; more than ten busy threads drop its clock to about 1.1 GHz. Each
program builds its input outside the timed region, times one terminal with the
monotonic clock per repetition, discards the first repetition and prints a
checksum; every variant of a case must agree. The script interleaves the
variants over three rounds of four timed repetitions and reports medians in
milliseconds. The machine was not otherwise idle (load average 2 to 5 from
other work), so differences under about 10% are noise.

**Screening.** Before timing, `--dump asm` of the chunk workers showed the
same per-item calls as the serial loop (`plenty_list_scalar_get` and, when
collecting, `plenty_collection` for the append) for `map`, `filter` and
`count`. The signed-sum worker folded into a seven-field tuple and called
`signed_step` per item, with two `memmove` calls for the tuple state; a serial
microbenchmark of that fold over ten million items took 400 ms against 20 ms
for the same arithmetic in local variables and 6 ms for `sum`. The worker now
runs the chunk in a loop over locals, tracking the exact running total as a
wrapping `i64` plus a wrap count. Combining collected chunks now appends each
chunk's storage with `extend` instead of moving items one at a time.

The first round of measurements found two losses: splitting a list moved each
item on the calling thread with a list append (about 37 ns per item), and
`.par` `reduce` collected every item, then ran `reduce_tree`, which submitted
one job per pair. Three changes followed:

- A list source was split by an internal list operation that copied each
  chunk's contiguous rows in one bulk copy. Issue #455 replaced this with borrowed chunks, described in this report.
- `pool.reduce_tree` gives one job an aligned run of adjacent inputs, a power
  of two chosen so there is about one run per worker, and the job reduces it
  serially through the same rounds. The remaining run results are combined by
  pair jobs. Because each round maps index `i` to `i / 2`, an aligned run is
  exactly a subtree of the fixed tree, so the grouping is unchanged; a test
  compares it with a serial level-by-level evaluation for lengths 0 to 70 and
  larger odd and even lengths, with 1, 2, 3 and 8 workers.
- `.par` `reduce` now groups like `sum`: each chunk reduces its items left to
  right in its worker, and the chunk results are reduced in input order. No
  list of all items is collected. The worker's loop makes the same direct calls
  per item as the serial pipeline (`plenty_list_scalar_get`, the callback and
  `f`).

**Borrowed chunks (issue #455).** A list chunk is now a span of the source's
rows: the job receives the first row's address, the row size and the item
count, and reads (or, for owned affine items, takes) items in place. The
calling thread copies nothing; it allocates one small list of chunk bounds.
This relies on every terminal draining its jobs before it returns. `--dump asm`
of the `filter`+`count` worker showed the per-item loop with no calls (an
`imul` and a load per row, then the inlined predicate); before, it called
`plenty_list_scalar_get` per item, and the split called the runtime copy.

The first measurement after that change regressed `filter`+`count` and the
sums at 100,000 items (0.46 to 1.24 times serial): the owned source was now
dropped on the calling thread, and dropping a list walked and cleared every
row even when the items own nothing (about 3.4 ns per row). A list of items
that own no heap storage now frees its buffer without the walk. That also
makes the serial pipelines, which consume the list, faster.

Results on an AMD Ryzen 7 7840HS, eight workers, with borrowed chunks and the
drop change (load average 5 to 6 from other work):

| Case | Callback | Items | Serial | pool | par | par/serial | par/pool |
|---|---|---:|---:|---:|---:|---:|---:|
| map+collect | cheap | 1,000 | 0.041 | 1.082 | 0.042 | 1.03 | 0.04 |
| map+collect | cheap | 100,000 | 5.213 | 88.015 | 3.313 | 0.64 | 0.04 |
| map+collect | cheap | 10,000,000 | 567.259 | 9362.107 | 328.201 | 0.58 | 0.04 |
| map+collect | heavy | 1,000 | 0.340 | 1.142 | 0.129 | 0.38 | 0.11 |
| map+collect | heavy | 100,000 | 34.433 | 97.323 | 9.094 | 0.26 | 0.09 |
| map+collect | heavy | 1,000,000 | 331.989 | 982.120 | 74.592 | 0.22 | 0.08 |
| filter+count | cheap | 1,000 | 0.006 | 1.035 | 0.013 | 2.19 | 0.01 |
| filter+count | cheap | 100,000 | 0.637 | 104.234 | 0.082 | 0.13 | 0.00 |
| filter+count | cheap | 10,000,000 | 84.430 | 9071.530 | 31.855 | 0.38 | 0.00 |
| filter+count | heavy | 1,000 | 0.271 | 0.717 | 0.080 | 0.30 | 0.11 |
| filter+count | heavy | 100,000 | 27.919 | 90.143 | 3.902 | 0.14 | 0.04 |
| filter+count | heavy | 1,000,000 | 274.661 | 883.825 | 34.612 | 0.13 | 0.04 |
| sum i64 | cheap | 1,000 | 0.005 | 0.809 | 0.012 | 2.25 | 0.01 |
| sum i64 | cheap | 100,000 | 0.544 | 89.318 | 0.089 | 0.16 | 0.00 |
| sum i64 | cheap | 10,000,000 | 80.116 | 10037.634 | 36.010 | 0.45 | 0.00 |
| sum i64 | heavy | 1,000 | 0.282 | 0.812 | 0.089 | 0.32 | 0.11 |
| sum i64 | heavy | 100,000 | 28.724 | 108.087 | 6.420 | 0.22 | 0.06 |
| sum i64 | heavy | 1,000,000 | 282.463 | 969.470 | 40.324 | 0.14 | 0.04 |
| sum i64 range | cheap | 1,000 | 0.001 | 0.810 | 0.016 | 23.21 | 0.02 |
| sum i64 range | cheap | 100,000 | 0.068 | 90.465 | 0.079 | 1.16 | 0.00 |
| sum i64 range | cheap | 10,000,000 | 6.585 | 9071.521 | 5.721 | 0.87 | 0.00 |
| sum i64 range | heavy | 1,000 | 0.308 | 0.892 | 0.100 | 0.33 | 0.11 |
| sum i64 range | heavy | 100,000 | 27.919 | 91.461 | 4.882 | 0.17 | 0.05 |
| sum i64 range | heavy | 1,000,000 | 276.006 | 929.201 | 38.852 | 0.14 | 0.04 |
| sum f64 | cheap | 1,000 | 0.005 | 0.836 | 0.010 | 1.82 | 0.01 |
| sum f64 | cheap | 100,000 | 0.547 | 95.449 | 0.046 | 0.08 | 0.00 |
| sum f64 | cheap | 10,000,000 | 75.071 | 9511.754 | 30.247 | 0.40 | 0.00 |
| sum f64 | heavy | 1,000 | 0.103 | 0.827 | 0.034 | 0.33 | 0.04 |
| sum f64 | heavy | 100,000 | 10.583 | 92.945 | 1.673 | 0.16 | 0.02 |
| sum f64 | heavy | 1,000,000 | 104.262 | 919.342 | 13.321 | 0.13 | 0.01 |
| reduce | cheap | 1,000 | 0.005 | 0.850 | 0.011 | 2.09 | 0.01 |
| reduce | cheap | 100,000 | 0.596 | 90.544 | 0.064 | 0.11 | 0.00 |
| reduce | cheap | 10,000,000 | 76.897 | 9112.809 | 29.458 | 0.38 | 0.00 |
| reduce | heavy | 1,000 | 0.270 | 0.772 | 0.076 | 0.28 | 0.10 |
| reduce | heavy | 100,000 | 27.307 | 88.855 | 3.867 | 0.14 | 0.04 |
| reduce | heavy | 1,000,000 | 272.071 | 897.812 | 34.352 | 0.13 | 0.04 |

Before, at main `9e3970d5` with the bulk-copy split, measured just before in
the same conditions (load average 4 to 6):

| Case | Callback | Items | Serial | pool | par | par/serial | par/pool |
|---|---|---:|---:|---:|---:|---:|---:|
| map+collect | cheap | 1,000 | 0.051 | 0.763 | 0.044 | 0.86 | 0.06 |
| map+collect | cheap | 100,000 | 6.016 | 98.522 | 2.838 | 0.47 | 0.03 |
| map+collect | cheap | 10,000,000 | 641.972 | 11471.189 | 540.434 | 0.84 | 0.05 |
| map+collect | heavy | 1,000 | 0.348 | 0.668 | 0.137 | 0.39 | 0.20 |
| map+collect | heavy | 100,000 | 35.873 | 98.538 | 6.967 | 0.19 | 0.07 |
| map+collect | heavy | 1,000,000 | 358.055 | 1089.048 | 71.763 | 0.20 | 0.07 |
| filter+count | cheap | 1,000 | 0.010 | 0.812 | 0.026 | 2.62 | 0.03 |
| filter+count | cheap | 100,000 | 1.115 | 134.447 | 0.738 | 0.66 | 0.01 |
| filter+count | cheap | 10,000,000 | 136.069 | 15679.961 | 262.102 | 1.93 | 0.02 |
| filter+count | heavy | 1,000 | 0.322 | 1.182 | 0.105 | 0.33 | 0.09 |
| filter+count | heavy | 100,000 | 31.387 | 145.978 | 8.457 | 0.27 | 0.06 |
| filter+count | heavy | 1,000,000 | 312.911 | 1371.325 | 63.929 | 0.20 | 0.05 |
| sum i64 | cheap | 1,000 | 0.010 | 0.921 | 0.033 | 3.31 | 0.04 |
| sum i64 | cheap | 100,000 | 0.980 | 132.005 | 0.821 | 0.84 | 0.01 |
| sum i64 | cheap | 10,000,000 | 129.799 | 12216.945 | 248.014 | 1.91 | 0.02 |
| sum i64 | heavy | 1,000 | 0.318 | 0.776 | 0.090 | 0.28 | 0.12 |
| sum i64 | heavy | 100,000 | 29.457 | 114.188 | 6.868 | 0.23 | 0.06 |
| sum i64 | heavy | 1,000,000 | 317.642 | 1362.148 | 69.637 | 0.22 | 0.05 |
| sum i64 range | cheap | 1,000 | 0.002 | 2.140 | 0.054 | 27.75 | 0.03 |
| sum i64 range | cheap | 100,000 | 0.169 | 156.296 | 0.164 | 0.97 | 0.00 |
| sum i64 range | cheap | 10,000,000 | 9.379 | 11315.101 | 5.753 | 0.61 | 0.00 |
| sum i64 range | heavy | 1,000 | 0.265 | 0.810 | 0.083 | 0.31 | 0.10 |
| sum i64 range | heavy | 100,000 | 26.573 | 92.517 | 5.245 | 0.20 | 0.06 |
| sum i64 range | heavy | 1,000,000 | 268.563 | 895.409 | 37.141 | 0.14 | 0.04 |
| sum f64 | cheap | 1,000 | 0.009 | 0.769 | 0.017 | 1.89 | 0.02 |
| sum f64 | cheap | 100,000 | 0.922 | 92.612 | 0.466 | 0.51 | 0.01 |
| sum f64 | cheap | 10,000,000 | 113.042 | 9285.398 | 208.312 | 1.84 | 0.02 |
| sum f64 | heavy | 1,000 | 0.121 | 0.892 | 0.047 | 0.39 | 0.05 |
| sum f64 | heavy | 100,000 | 11.289 | 100.961 | 3.131 | 0.28 | 0.03 |
| sum f64 | heavy | 1,000,000 | 114.782 | 950.894 | 19.875 | 0.17 | 0.02 |
| reduce | cheap | 1,000 | 0.009 | 0.777 | 0.021 | 2.24 | 0.03 |
| reduce | cheap | 100,000 | 1.021 | 105.988 | 0.694 | 0.68 | 0.01 |
| reduce | cheap | 10,000,000 | 119.967 | 9456.671 | 202.308 | 1.69 | 0.02 |
| reduce | heavy | 1,000 | 0.313 | 0.834 | 0.083 | 0.27 | 0.10 |
| reduce | heavy | 100,000 | 28.515 | 91.681 | 4.281 | 0.15 | 0.05 |
| reduce | heavy | 1,000,000 | 279.319 | 911.116 | 39.372 | 0.14 | 0.04 |

**Findings.**

- With cheap callbacks over ten million list items, `.par` now takes 0.38 to
  0.58 times the serial time (1.7 to 1.9 before). At 100,000 items it takes
  0.08 to 0.16 times serial for `filter`+`count`, the sums and `reduce`, and
  0.64 for `map`+`collect`, which still appends each result.
- With expensive callbacks, `.par` runs at 0.13 to 0.26 times the serial time
  from 100,000 items, as before.
- At 1,000 cheap items the job overhead still exceeds the work (1.8 to 2.3
  times serial). A range source over cheap items is unchanged.
- `reduce` keeps per-chunk grouping; `pool.reduce_tree` keeps aligned runs.

**Remaining work.** Early stop for parallel `any`/`all`; user-facing scoped
pool jobs (a deferred B22 extension) could reuse the drain-before-return
guarantee.

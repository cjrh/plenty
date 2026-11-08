# Bitpacked clist columns

Issue #655: compares counting `v > t` over a native `u32` column (scalar and
SSE2), narrow native `u16`/`u8` columns, a native column with per-block
min/max, and 128-row frame-of-reference and delta bitpacked blocks in
simdcomp's 4x layout. It also measures codec and mutation costs. `bench.c`
uses SSE2 intrinsics only; `scan.plenty` measures Plenty's current filter/count
paths on `i32`, `u16` and `u8` columns.

```sh
cargo build --locked --release --bin plenty
python3 -B experiments/clist-bitpacking/run.py target/release/plenty --out target/bitpack
python3 -B experiments/clist-bitpacking/run.py --tables experiments/clist-bitpacking/results.json
```

Requires Linux x86-64, gcc, `taskset` and `perf` access. `--quick` runs only the
cache-resident size; the full run allocates about 1.5 GiB and takes a few
minutes. Every encoding is round-tripped and every scan is checked against the
scalar count, so a mismatch exits with status 2.

Instructions per row come from `perf_event_open` around the measured scans.
Plenty counts subtract a `REPS = 1` build from a `REPS = 101` build. Elapsed
times are the best of five trials, pinned to `--cpus`. `results.json` records
the host load average and running `rustc` count before and after; the retained
run was taken under a load average of 14–37 from concurrent builds, so its
times are slower than in the report. `first-run.jsonl` holds the raw lines of
the earlier run (load average 8–11) whose times the report quotes. See the
[report](../../book/src/proposals/columnar-list/bitpacking.md).

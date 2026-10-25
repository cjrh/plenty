# Optional Polars embedding spike

The original isolated experiment below is retained as historical evidence.
The supported `std.dataframe` module now lives in `stdlib/std/dataframe.plentyi`
with its optional engine in `plenty-dataframe/`; it keeps `clist` intact.
The final-module measurements at the end of this page use that implementation.
The original adapter in this directory is not the supported stdlib API.
It links a Plenty executable to a prebuilt Rust `cdylib` containing Polars
0.55.2. No compiler, runtime, root Cargo dependency, or normal build changes.

Read the design and evidence in `book/src/proposals/polars-dataframes.md` and
`book/src/proposals/polars-dataframes/evidence.md` from the repository root.

## Reproduce

Requirements: Linux x86-64-v3 hardware, Rust 1.99, Python 3, GNU time, `cc`, mold,
and a release Plenty compiler built from this checkout without runtime checks.
Build the compiler from the repository root with
`cargo build --locked --release --bin plenty` (an isolated `CARGO_TARGET_DIR`
is recommended when other workers are active). Commands below run here.

```sh
python3 -B build.py --target /absolute/new/polars-target --clean
python3 -B run.py --compiler /absolute/path/to/plenty \
  --cargo-target /absolute/new/polars-target --out /absolute/spike-results \
  --cpus 2,4,6,8
```

Choose four distinct physical cores on your machine; the example fits the
recorded Ryzen host. `--clean` refuses to reuse an existing directory. Omit it
to measure a dependency-warm adapter rebuild. `build.py` records explicit flags,
build output, GNU time resource usage and Rust version under the target. Polars
is built once; `run.py` never invokes Cargo. Its `--checks-only` option skips
performance trials. Every timing case checks its mathematical checksum.

For Rust validation, use the same target and flags as `build.py` to reuse builds:

```sh
export CARGO_TARGET_DIR=/absolute/new/polars-target
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc
export RUSTFLAGS='-C target-cpu=x86-64-v3 -C link-arg=-fuse-ld=mold -C link-arg=-Wl,--thread-count=3'
cargo fmt --all --check
cargo test --locked --release --workspace -j 3
cargo clippy --locked --release --all --workspace --all-targets -j 3 -- -D warnings
```

`run.py` builds the example, ABI checks, two benchmark programs and two startup
programs. It copies the shared engine beside the executable and uses `$ORIGIN`
for runtime lookup. The Rust baseline is `src/bin/direct.rs`. The engine has a
separate allocator and owns its handles; Plenty never frees a Rust allocation.
The trusted `.plentyi` facade releases each owner through its matching function.
Numeric values cross by copy; there is no claim of zero-copy interoperability.

Inputs/outputs and build products stay under the output/target directories.
`observed-results.json` and `observed-build.json` preserve the measured run.

## Final std.dataframe foundation

Run from the repository root, after other builds/tests finish:

```sh
python3 -B scripts/build_dataframe_engine.py --out target/release
python3 -B experiments/polars-embedding/measure_std_dataframe.py \
  --compiler target/release/plenty \
  --engine target/release/libplenty_dataframe_v1.so \
  --cpus 2,4,6,8 --out experiments/polars-embedding/observed-stdlib.json
```

Select four distinct physical cores available to the process; the runner checks
the topology rather than accidentally using SMT siblings. It records five
process samples per case, 100 collected queries per process after five warmups,
and validates checksums. These results use the baseline x86-64 engine, not the
original spike's v3 flags. No Cargo compilation occurs during measurement.

Final recorded medians on the same Ryzen host:

| Rows | Polars threads | Plenty input lists | Native import, two columns | Grouped query | Export one f64 column |
|---|---:|---:|---:|---:|---:|
| 1,024 | 1 | 0.104 ms | 0.081 ms | 0.088 ms | 0.041 ms |
| 1,024 | 4 | 0.105 ms | 0.069 ms | 0.111 ms | 0.045 ms |
| 1,000,000 | 1 | 105.9 ms | 16.0 ms | 5.43 ms | 51.9 ms |
| 1,000,000 | 4 | 104.7 ms | 16.2 ms | 4.22 ms | 51.6 ms |

Compile/link medians are 82.9 ms without caches and 45.6 ms with the program
cache warm. The benchmark executable is 3,390,240 bytes; its shared engine is
93,547,752 bytes (89.2 MiB), not embedded in each executable. A final whole-process
`perf stat` run counts 10.68 billion user instructions and 3.74 billion user cycles.
That includes input allocation, copying, warmups, 100 queries, export and cleanup;
it is not a query-only counter or proof of speedup.

Host timing varies: earlier identical copying-constructor runs gave roughly
2.6–3.3 ms for the four-thread large query; the final five samples span
4.05–4.60 ms. Do not compare these wall times as a precise speedup over the
historical clist or direct-Rust runs. The bounded result is that compilation
still reuses the engine, per-element FFI has been removed, and conversion
remains important. Input lists and output-list initialization dominate the
boundary costs for a single short query.

Import currently packs strided Plenty values into a Vec, then uses Polars's
ordinary copying Series constructor. A two-line alternative using
`Series::from_vec` eliminated the second copy and reduced large import to
about 9.5 ms, but its single-thread query repeatedly rose to about 9.9 ms
(four-thread 4.5–4.8 ms). `observed-owned-buffer.json` preserves one such run.
The supported code retains the copying constructor. These measurements do not
establish whether buffer layout, allocation addresses or CPU interference
explain the difference. We did not add alignment machinery to this foundation.

Zero-copy interchange would need packed compatible element buffers, validity
bitmaps for nulls, stable ownership/lifetimes, and matching allocator callbacks;
ordinary Plenty lists have strided runtime slots. Current copies keep that
boundary simple and safe. Bulk export allocates initialized Plenty slots before
the native write, which explains why it is not just a memcpy benchmark.

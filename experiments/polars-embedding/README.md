# Optional Polars embedding spike

This is an isolated experiment, not `std.dataframe` and not a change to `clist`.
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

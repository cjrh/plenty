# Columnar-list feasibility spike

Native Rust layout and ownership experiments plus a concrete Plenty wrapper.
No production `clist` is implemented. The design and recorded results are in
`book/src/proposals/columnar-list.md` and its `evidence.md` companion.

From this directory:

```sh
cargo test --locked --workspace
cargo test --locked --workspace --release
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
python3 run.py --cpu 4
```

Choose an allowed Linux CPU or omit `--cpu`. The runner uses only Python's
standard library and requires `cargo`, `rustc`, `lscpu`, and (when pinning)
`taskset`. It overrides Rust flags deliberately for native and vectorization-
disabled builds, emits assembly under `target/{native,scalar}/release/deps/`,
and retains all seven timing samples, median/min/max summaries and hardware/
toolchain metadata under ignored `out/`. It checks every timed kernel result.
The crate has its own workspace and no external dependencies.

From the repository root, build the current compiler and check the Plenty
feasibility probes:

```sh
cargo build --locked --bin plenty
python3 experiments/columnar-list/check_plenty.py target/debug/plenty
```

`columns.plenty` uses existing scalar list fields and reserve-before-append,
then borrowed `zip`/`fold`; expected output is `90`. The other two probes must
reject unsupported `clist` and reference-containing classes. These baseline
rejections are not production regression tests or permanent feature gates.

The native workload is eight `u64` fields with explicit wrapping arithmetic.
The AoS record is 64 bytes, unlike Plenty's current generic collection slots.
Timed scans exclude all construction and allocation. All four layouts remain
allocated, samples rotate layout order, and scans repeat warm. The all-field
kernel mixes every field within a row; the permuted kernel visits all rows in
an odd-stride permutation. The isolated ownership model uses safe `Option`
slots and deterministic failure gates around actual `try_reserve` calls.
It tests correctness, not allocator failure coverage or mutation throughput.

For an adoption benchmark, add cold scans, hardware counters, representative
classes and checked/float operations, a quieter machine, growth and deletion
workloads, and end-to-end Plenty/Cranelift kernels. This experiment deliberately
does not prescribe the public chunk size or establish backend superiority.

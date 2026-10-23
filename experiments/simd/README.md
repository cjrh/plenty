# Portable SIMD proposal probes

Isolated evidence for the [SIMD proposal](../../book/src/proposals/std-simd.md).
No production compiler changes or SIMD API are implemented. Run on x86-64 Linux
with Rust, Cargo, Clang, a C linker, and GNU objdump. AArch64 is cross-compiled,
not executed. The locked dependencies must already be cached for `--offline`;
omit that flag in the runner's build command if intentionally fetching them.

From this directory:

```sh
cargo fmt --all -- --check
cargo clippy --offline --locked --all --workspace -- -D warnings
cargo test --offline --locked --all --workspace
python3 run.py --out /tmp/plenty-simd-results --plenty ../../target/release/plenty
```

Build the Plenty binary in this worktree first with
`cargo build --release --locked --bin plenty` from the repository root.
Omit `--plenty` to run backend-only probes. The Rust package intentionally has
its own workspace/lockfile; `cargo test` compiles the probe but contains no Rust
unit tests. The meaningful differential checks are the native C harness and
the Plenty result assertion run by `run.py`.

`src/main.rs` emits scalar, four-lane and eight-lane object probes for two target
triples. It uses raw pointers and requires lengths to be lane multiples; it is
not a safe public API. `driver.c` supplies disjoint valid arrays and scalar tails.
Do not call the generated vector functions on an arbitrary length directly.
`kernels.c` provides scalar-source LLVM comparisons, with contraction disabled.
`pipelines.plenty` validates the fluent-chain scalar oracles.

The runner captures objects, assembly, vectorization remarks, errors, native
checks, benchmarks, and command elapsed times in the selected output directory.
Eight-lane failure is recorded rather than hiding the unsupported operation.
Generated artifacts and build output are not committed. See the
[evidence page](../../book/src/proposals/std-simd/evidence.md) for the measured
environment, interpretation and limitations; microbenchmarks are not a backend
migration decision by themselves.

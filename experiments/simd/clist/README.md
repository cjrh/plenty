# Actual Plenty column filter/count

`scan.plenty` compares the same `i32 > 5` operation with an eligible constant
predicate and an explicit scalar column loop. `threshold.plenty` uses a runtime
captured threshold, and `range.plenty` uses two runtime captured bounds joined
by `and`. All use the production compiler and `--release`. The scalar reference
has no per-row callback call, so it does not inflate gains with callback overhead.
The original reports used a captured threshold to force scalar execution; that
is now eligible for SIMD, so the current runner deliberately uses a loop instead.

```sh
cargo build --locked --release --bin plenty
python3 experiments/simd/clist/run.py target/release/plenty --out /tmp/clist-simd --verify-only
python3 experiments/simd/clist/run.py target/release/plenty --out /tmp/clist-simd
python3 experiments/simd/clist/run.py target/release/plenty --scenario threshold --out /tmp/clist-threshold
python3 experiments/simd/clist/run.py target/release/plenty --scenario range --out /tmp/clist-range
```

Requires Linux x86-64, Python, a C compiler, GNU binutils and (for timings) `perf`
access. `--cpu` chooses a logical CPU; the default is 8. The verification-only
mode needs no performance counters and runs in the integration suite.

The runner captures the exact release object/runtime at the linker boundary,
globalizes two internal scan symbols and renames the unused runtime `main`.
It gives Plenty's function sections a separate output section, so unrelated
runtime section alignment cannot collapse the requested placements; the actual
SIMD function address modulo 64 is asserted for each executable.
`harness.c` calls these functions using a test-only internal ABI. It creates
borrowed, read-only owner headers with their data against a protected page;
it never asks the runtime to mutate or destroy them. Odd lengths need four
poison bytes to align the header, so scalar-oracle comparison supplements the
guard. This is not an example of supported C interoperation.

The benchmark links the one object at four text addresses, 16 bytes apart,
and shuffles scalar/SIMD runs at lengths 7, 8,192 and 262,144. It retains three
samples after a warmup. Internal wall timing covers repeated scans; performance
counters include process startup and constructing the fixture. Do not run it
alongside compiler builds. Raw reports, dumps, objects and executables remain
in the output directory; the retained final report is `measurements.json`.
The earlier 0.131.1 report is preserved separately as
`measurements-cranelift-0.131.json`; it is not the current compiler's result.

To check the pinned backend's vector-width boundary independently of Plenty's
pattern recognition, compile the small workspace example and run each mode:

```sh
cargo build --locked --release --example clist_simd_backend_probe
target/release/examples/clist_simd_backend_probe 4 baseline /tmp/width4.o
target/release/examples/clist_simd_backend_probe 4 native /tmp/width4-native.o
target/release/examples/clist_simd_backend_probe 8 native /tmp/width8-native.o
```

This is a compile-only load/compare/store probe with the verifier enabled.
`native` enables detected host features for that probe only. Repeat with 8 and
16 lanes in both modes to distinguish 128-bit support from 256/512-bit support;
unsupported widths return a compilation error. It does not change Plenty's
target policy or claim execution support for a rejected width.

See [the report](../../../book/src/proposals/columnar-list/simd-counts.md)
for results and interpretation.

The [runtime-bound extension report](../../../book/src/proposals/columnar-list/simd-runtime-bounds.md)
describes the new scenarios. Their retained reports are
`measurements-runtime-threshold.json` and `measurements-runtime-range.json`;
they use the explicit-loop reference and must not be mixed with the earlier
constant-only results.

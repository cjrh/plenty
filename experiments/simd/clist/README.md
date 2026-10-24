# Actual Plenty column filter/count

`scan.plenty` compares the same `i32 > 5` operation with an eligible constant
predicate and a captured threshold that retains scalar execution. Both use
the production compiler and `--release`. The scalar threshold is held in a
register in the loop; its capture adds fixed setup, which matters for tiny scans.

```sh
cargo build --locked --release --bin plenty
python3 experiments/simd/clist/run.py target/release/plenty --out /tmp/clist-simd --verify-only
python3 experiments/simd/clist/run.py target/release/plenty --out /tmp/clist-simd
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

See [the report](../../../book/src/proposals/columnar-list/simd-counts.md)
for results and interpretation.

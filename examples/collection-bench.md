# Collection access benchmarks

Run from the repository root with a release compiler:

```sh
cargo build --release --offline
python3 examples/collection_bench.py --compiler target/release/plenty --output /tmp/collection-bench --runs 5
```

The script generates standalone Plenty sources and native executables, checks
each checksum, and saves `results.json` with compiler SHA-256, platform, options,
input sizes, separate compilation timings, and every execution timing. Execution
includes process startup and construction; compilation includes native linking.
Use separate output directories and compiler binaries for before/after runs.
There are no timing assertions in ordinary tests.

The baseline is local `main` at `509bda2`. Both compilers use Cargo's release
profile, Cranelift 0.131.3, the default native compiler options (`--compile FILE
-o OUT`), and the packaged Rust runtime (`opt-level=2`, thin LTO, one codegen
unit). Measurements were taken on 2026-10-10, Linux x86-64, AMD Ryzen 7 7840HS,
rustc 1.99.0 (b940084d7 2026-09-28), Rust configuration
`target-cpu=x86-64-v3`. The benchmark records the actual compiler binary hash
because source revision alone does not capture compiler configuration.

Five runs per executable, median execution times in milliseconds; compilation
was measured once for each generated input, also in milliseconds. Both builds
have runtime allocation instrumentation disabled. Every checksum matched.

| Workload | Iterations/elements | Compile before / after | Run before / after | Checksum |
|---|---:|---:|---:|---:|
| Scalar read | 2,000,000 | 57.5 / 52.1 | 29.15 / 19.36 | 1022942656 |
| Scalar read | 8,000,000 | 52.3 / 59.6 | 111.10 / 75.13 | 4091868928 |
| Scalar write | 2,000,000 | 51.8 / 53.7 | 29.70 / 29.63 | 1999999 |
| Scalar write | 8,000,000 | 52.4 / 64.2 | 106.70 / 110.57 | 7999999 |
| Length | 2,000,000 | 53.5 / 61.6 | 22.50 / 9.76 | 2048000000 |
| Length | 8,000,000 | 51.6 / 61.1 | 85.92 / 34.69 | 8192000000 |
| Nested read | 200,000 | 53.8 / 57.6 | 6.87 / 4.43 | 3100000 |
| Nested read | 800,000 | 54.0 / 82.3 | 24.07 / 14.74 | 12400000 |
| Hash hit/miss | 200,000 | 51.2 / 55.7 | 4.50 / 4.84 | 100352 |
| Hash hit/miss | 800,000 | 53.5 / 60.9 | 15.64 / 16.24 | 400384 |
| Repeated front removal | 2,000 | 52.5 / 63.7 | 7.97 / 8.92 | 1999000 |
| Repeated front removal | 8,000 | 55.3 / 66.5 | 109.08 / 134.76 | 31996000 |

The short runs are sensitive to scheduling/startup. An additional eight-pair
run alternating before/after order measured 114.54 / 73.77 ms for 8M reads,
85.28 / 34.21 ms for 8M lengths, 23.88 / 14.62 ms for 800K nested reads, and
111.00 / 116.32 ms for 8K front removals. The read/length improvements persist;
front-removal results are variable and do not show an improvement. No speedup
is claimed for writes or hash probes. Compilation measurements include linking
noise and are not evidence of a compilation-speed improvement.

Compiler SHA-256 values for the recorded comparison:

```text
before 6204a0bf3d6a13afb2b9f7b7a42b319bc2fa44992f7e7a24b2fcc04864a81185
after  7f09194ca35bd9164da555d51675b889cda6b167238a99122f6dc3aa7f6f60c9
```

The narrow list helpers still make an out-of-line call and access type-sized
runtime rows. This change removes dispatcher packing and borrowed-receiver
refcount calls; it does not implement a public layout for inline loads. Scalar
writes and hash probes retain their existing paths. Repeated front removal
remains quadratic; comparisons at 2,000 and 8,000 elements expose that cost.
No claim of improvement over CPython is made by these measurements.

`tests/test_list_fast_paths.rs` checks relocations for a function containing
only borrowed scalar indexing and length: it must call both narrow helpers and
must not reference `plenty_collection`, `plenty_retain`, or `plenty_release`.
An `objdump -dr` comparison for `xs[i] + len(xs)` confirmed two dispatcher,
two retain, and two release calls before, versus just the two narrow collection
helpers after. Both versions retain the arithmetic overflow trap for addition.
To inspect native assembly manually, compile the generated source with
`--emit-object FILE -o access.o`, then run `objdump -dr access.o`. Main also
contains construction/printing/cleanup calls, so inspect the loop or isolated
function rather than counting all runtime calls in a whole application.

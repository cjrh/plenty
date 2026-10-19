# Collection access benchmarks

## Keyed hashing (issue #329, 2026-10-19)

The runtime replaces unkeyed FNV/integer mixing with SipHash-2-4 keyed from
OS entropy once per process. This is security hardening with a measurable
short-key cost. It does not promise worst-case constant-time table operations.

Compared local `main` at `41f3f6c` with the issue #329 change using release
compilers, rustc 1.99.0 (b940084d7 2026-09-28), Linux x86-64, AMD Ryzen 7 7840HS,
and `target-cpu=x86-64-v3`. Runtime allocation instrumentation was disabled.
Each workload ran seven times per executable, alternating before/after order,
pinned to CPU 0 with `PLENTY_CACHE=off`. Every checksum matched across all 20
workloads. Execution includes startup and construction; the lookup workloads
build query strings once before their lookup loops. The harness saves raw
timings, compiler hashes, affinity, and load metadata in `results.json`.
The local run's copy is `/tmp/plenty-329-paired/results.json`.

| Hash workload | Iterations/elements | Before median ms | After median ms |
| --- | ---: | ---: | ---: |
| Integer hit/miss | 200,000 | 4.757 | 6.749 |
| Integer hit/miss | 800,000 | 16.653 | 24.563 |
| Short string hit/miss | 200,000 | 17.697 | 20.946 |
| Short string hit/miss | 800,000 | 43.377 | 54.615 |
| Long string hit/miss | 200,000 | 67.283 | 63.491 |
| Long string hit/miss | 800,000 | 120.300 | 111.497 |
| `(i64, str)` hit/miss | 200,000 | 27.817 | 32.140 |
| `(i64, str)` hit/miss | 800,000 | 85.205 | 97.565 |
| Structured string construction/lookup | 2,000 | 2.744 | 2.548 |
| Structured string construction/lookup | 8,000 | 8.724 | 7.955 |

At 800,000 queries, integer lookup cost increased about 47%, short strings
26%, and tuples 15%. Longer strings measured about 7% lower. The structured
keys enumerate fixed-length combinations of `a`, `q`, `A`, `Q`, `1`, and `!`:
their old FNV hashes share the low four bits. This workload measures that
clustering pattern; it is not a proof or benchmark of resistance to a capable
hash-flooding attacker. The fixed-key runtime regression separately checks the
six original one-byte examples distribute across multiple buckets.

Unrelated host workloads ran during this session. Alternating execution and
CPU affinity reduce some variation, but these measurements are illustrative,
not a general speedup claim. The initial 1/5/15-minute load averages were
2.81/16.45/20.29. Both binaries were checked before timing: the baseline lacks
the new entropy diagnostic, while the candidate embeds it in both native
runtime archives and links the new vendored SipHash implementation.

The AOT check used `compile_bench 1000 10 --workload aot`, warmup enabled,
cache off, pinned to CPUs 0–7, ordered before/after/after/before. Each value is
a median of ten compilations including checking, archive work, and linking:
before **525.135 / 526.114 ms**, after **528.751 / 532.738 ms**. No compilation
speedup is claimed. Full local commands and outputs are preserved in
`/tmp/plenty-329-aot-results.json`.

Compiler SHA-256 values:

```text
before ce6d03826e46203857a4936b13adcfd18b0d019046e632ddfbd836195df5f373
after  4eb842b88e3923c272349072ac68da2895ee94731750cb2ee33002096098249e
```

To reproduce paired collection measurements with two preserved compiler
binaries:

```sh
PLENTY_CACHE=off taskset -c 0 python3 examples/collection_bench.py --compiler /path/to/after --baseline-compiler /path/to/before --output /tmp/hash-comparison --runs 7
```

## Earlier collection access measurements

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

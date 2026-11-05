# Benchmarks Game suite

`benchmarks/game/` holds ten programs from the
[Computer Language Benchmarks Game](https://benchmarksgame-team.pages.debian.net/benchmarksgame/)
with a Plenty version of each, so that changes to generated code can be
measured against fixed programs and a recorded baseline. It is a regression
reference: it records where Plenty stands, and a slower run after a compiler
change is a finding to explain. To make a program faster, follow
[Analyzing emitted code](emitted-code.md).

## Layout

Each benchmark directory contains:

| Path | Contents |
|---|---|
| `benchmark.json` | Provenance, site build and command lines, local build command, thread count, sizes, Plenty variants, and `status` (`ready` or `blocked` with `blocked_reason`) |
| `reference/` | The reference program, copied verbatim from its site page |
| `expected/` | Output at the small check size, from the site's downloadable output files |
| `plenty/` | The idiomatic Plenty program |

The reference programs are distributed under the Benchmarks Game's revised
BSD license, reproduced in `benchmarks/game/LICENSE-benchmarks-game.txt`.
Each manifest names the program's page and contributors.

| Benchmark | Reference | Threads | Plenty variants | Status |
|---|---|---|---|---|
| fannkuch-redux | C #1 | 1 | idiomatic; parallel (`examples/fannkuch_redux.plenty`, 8 workers) | ready |
| n-body | Rust #3 | 1 | idiomatic | ready |
| spectral-norm | C #8 | 1 | idiomatic | ready |
| mandelbrot | C #8 | 1 | idiomatic | ready |
| fasta | Rust #3 | 1 | idiomatic | ready |
| k-nucleotide | Rust #1 | 7 | idiomatic (1 thread) | ready |
| reverse-complement | C #5 | 1 | idiomatic | ready |
| binary-trees | C #1 | 1 | idiomatic | ready |
| pidigits | C #1 (GMP) | 1 | none | blocked: no arbitrary-precision integers (backlog B38, issue #481) |
| regex-redux | Rust #1 | 9 | idiomatic, with `std.regex` (1 thread) | ready |

Each reference is one C or Rust program from the site. None is marked with an
asterisk, the site's mark for a program that uses a different algorithm, so
each benchmark has one idiomatic Plenty program that follows the reference's
algorithm. Where the reference uses
threads and the Plenty program does not, the table says so; compare CPU time
(`user_s`) as well as wall time for those.

## Running

The runner, `benchmarks/game/bench.py`, uses only the Python standard
library. It needs the release compiler (`cargo build --locked --release`),
`clang`, `rustc` and `cargo` for the references, and optionally `perf`,
`taskset` and GNU `time`. regex-redux's reference is a Cargo package that
uses the `regex` version pinned in the workspace lockfile; it builds offline
from the local registry.

```sh
python3 -B benchmarks/game/bench.py list
python3 -B benchmarks/game/bench.py check                      # build all, verify check sizes
python3 -B benchmarks/game/bench.py run --mode quick n-body    # one benchmark, small size
python3 -B benchmarks/game/bench.py run --mode full --perf --output results.json \
    --compare benchmarks/game/baseline.json
python3 -B benchmarks/game/bench.py compare benchmarks/game/baseline.json results.json --metric instructions
```

Builds, generated inputs and outputs go under `target/bench-game/`. `run`:

1. builds the references with the commands in their manifests, which match the
   site's flags except `-march=native` (`-C target-cpu=native`) instead of the
   site's `ivybridge`;
2. builds each Plenty variant with `plenty --compile`;
3. checks every program's output at the check size against `expected/`;
4. generates the fasta inputs for k-nucleotide, reverse-complement and
   regex-redux with the fasta reference;
5. runs each program `--repeats` times (default 3), pinned with
   `taskset -c 0,2,4,6,8,10,12,14` (one thread per physical core on the
   reference laptop), recording wall time, CPU time and peak RSS, and reports
   the median and minimum;
6. compares each Plenty program's output at the timed size with the
   reference's output from the same run;
7. with `--perf`, adds one `perf stat` run for `instructions:u` and
   `cycles:u`;
8. writes JSON with the commit, tool versions, build commands and machine.

n-body and spectral-norm outputs compare within 1e-8; the others must be
byte-identical.

`compare` (or `run --compare`) reports each Plenty program whose metric grew
by more than `--threshold` (default 0.10) and exits 1 if any did. Metrics are
`time` (median wall time), `ratio` (median relative to the reference in the
same run, which transfers better between machines) and `instructions`
(the least noisy on one machine). Benchmarks run at different sizes are
skipped.

## Sizes and baseline

`quick` sizes finish in about half a minute in total. `full` sizes are the
site's except reverse-complement, which uses the 25,000,000 fasta input shared
with k-nucleotide (about 250 MB) instead of the site's 100,000,000 (about
1 GB). `benchmarks/game/baseline.json` holds a full run on the reference
laptop (AMD Ryzen 7 7840HS, performance governor):

| Benchmark | Size | Reference | Plenty | Ratio |
|---|---:|---:|---:|---:|
| binary-trees | 21 | 11.11 s | 50.38 s | 4.54 |
| fannkuch-redux | 12 | 24.64 s | 29.21 s | 1.19 |
| fannkuch-redux (parallel, 8 threads) | 12 | 24.64 s | 3.30 s | 0.13 |
| fasta | 25,000,000 | 1.49 s | 4.17 s | 2.81 |
| k-nucleotide | 25,000,000 | 11.50 s (7 threads) | 60.63 s | 5.27 |
| mandelbrot | 16,000 | 12.49 s | 12.65 s | 1.01 |
| n-body | 50,000,000 | 1.27 s | 42.89 s | 33.67 |
| pidigits | 10,000 | blocked | blocked | |
| regex-redux | 5,000,000 | 0.74 s (9 threads) | 59.70 s | 80.66 |
| reverse-complement | 25,000,000 | 0.36 s | 6.39 s | 17.91 |
| spectral-norm | 5,500 | 1.26 s | 3.78 s | 3.00 |

Times are median wall-clock seconds. The k-nucleotide and regex-redux
references are multithreaded while Plenty runs one thread; `user_s` in the JSON
gives CPU time (36.2 s for the k-nucleotide reference, 0.65 s for regex-redux).

## Blocked benchmarks

A benchmark whose Plenty program needs a missing feature keeps its reference,
expected output and sizes, with `"status": "blocked"` and the reason. The
runner prints the reason instead of timing it, and
`tests/test_benchmarks_game.rs` asserts it stays blocked with a phase-guard
comment naming the backlog item. pidigits waits on `std.bigint` (backlog B38,
issue #481). Its reference also needs GMP's `gmp.h` and `-lgmp`.

## Investigating a regression

`dump` writes the Plenty program's optimized Cranelift IR and assembly, and
the reference's disassembly, under `target/bench-game/dump/NAME/`:

```sh
python3 -B benchmarks/game/bench.py dump n-body --filter advance
```

From there, follow [Analyzing emitted code](emitted-code.md) and its
`emitted-code` agent skill (`.agents/skills/emitted-code/SKILL.md`): isolate
the hot loop as a kernel next to an equivalent C kernel, as in
`experiments/kernels/`, and attribute each extra instruction before changing
the compiler. `run --perf --metric instructions` confirms the result on the
whole program.

## Tests

`tests/test_benchmarks_game.rs` compiles every Plenty variant without
optimization and checks it at the check size, so a compiler change that breaks
a benchmark fails `cargo test`. It does not time anything or build the
references. `benchmarks/game/test_bench.py` tests the runner:

```sh
cargo test --locked --test integration test_benchmarks_game::
python3 -B -m unittest discover -s benchmarks/game -p 'test_*.py'
```

## Adding a benchmark

1. Copy the reference program verbatim from its site page into `reference/`
   and its site output into `expected/`.
2. Write `benchmark.json` following an existing one, then the Plenty program.
3. Add the benchmark to `ready_benchmarks!` (or `BLOCKED`) in
   `tests/test_benchmarks_game.rs` and to the table above.
4. Run `bench.py check`, then a full run, and update `baseline.json`.

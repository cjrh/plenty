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
| `benchmark.json` | Provenance, site build and command lines, local build command, thread count, sizes, Plenty variants with any compiler `flags`, and `status` (`ready` or `blocked` with `blocked_reason`) |
| `reference/` | The reference program, copied verbatim from its site page |
| `expected/` | Output at the small check size, from the site's downloadable output files |
| `plenty/` | The Plenty programs: the idiomatic one, and any parallel, data-layout or unchecked variant |

The reference programs are distributed under the Benchmarks Game's revised
BSD license, reproduced in `benchmarks/game/LICENSE-benchmarks-game.txt`.
Each manifest names the program's page and contributors.

| Benchmark | Reference | Threads | Plenty variants | Status |
|---|---|---|---|---|
| fannkuch-redux | C #1 | 1 | idiomatic; parallel (`examples/fannkuch_redux.plenty`, 8 workers) | ready |
| n-body | Rust #3 | 1 | idiomatic; clist (bodies in a `clist`); unchecked (`--unchecked assume`) | ready |
| spectral-norm | C #8 | 1 | idiomatic | ready |
| mandelbrot | C #8 | 1 | idiomatic | ready |
| fasta | Rust #3 | 1 | idiomatic | ready |
| k-nucleotide | Rust #1 | 7 | idiomatic (1 thread); parallel (7 pool workers) | ready |
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
2. builds each Plenty variant with `plenty --compile` and the variant's `flags`;
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
laptop (AMD Ryzen 7 7840HS, performance governor) at Plenty 0.8.18:

| Benchmark | Size | Reference | Plenty | Ratio |
|---|---:|---:|---:|---:|
| binary-trees | 21 | 11.61 s | 24.71 s | 2.13 |
| fannkuch-redux | 12 | 25.27 s | 24.68 s | 0.98 |
| fannkuch-redux (parallel, 8 threads) | 12 | 25.27 s | 2.83 s | 0.11 |
| fasta | 25,000,000 | 1.52 s | 2.56 s | 1.69 |
| k-nucleotide | 25,000,000 | 11.22 s (7 threads) | 32.25 s | 2.88 |
| k-nucleotide (parallel, 7 threads) | 25,000,000 | 11.22 s (7 threads) | 9.02 s | 0.80 |
| mandelbrot | 16,000 | 11.30 s | 12.01 s | 1.06 |
| n-body | 50,000,000 | 1.24 s | 4.91 s | 3.97 |
| n-body (clist) | 50,000,000 | 1.24 s | 6.10 s | 4.94 |
| n-body (unchecked) | 50,000,000 | 1.24 s | 3.47 s | 2.81 |
| pidigits | 10,000 | blocked | blocked | |
| regex-redux | 5,000,000 | 0.71 s (9 threads) | 3.42 s | 4.79 |
| reverse-complement | 25,000,000 | 0.32 s | 0.46 s | 1.46 |
| spectral-norm | 5,500 | 1.12 s | 2.10 s | 1.88 |

This is the recorded **0.8.18 baseline**, not a fresh full-suite run. The later
[guarded numerical-state measurement](../completed-work/performance.md#guarded-numerical-state-issue-930)
uses the unchanged n-body source at one million steps: 1,995.713M → 702.713M
instructions and 104.89ms → 51.82ms in its paired session. Its different size and
compiler version must not be substituted directly into the baseline table.

Times are median wall-clock seconds. The k-nucleotide and regex-redux
references are multithreaded while the idiomatic Plenty programs run one
thread; `user_s` in the JSON gives CPU time (35.8 s for the k-nucleotide
reference, 0.60 s for regex-redux).

k-nucleotide's parallel variant counts the seven frame lengths on seven pool
workers, as the reference does, and is the suite's threaded workload for
reference-count changes (issue #751): each job owns a `Shared` handle to the
sequence, and its dictionary, with heap-string keys for the 12- and 18-byte
frames, returns through a future and is released on the main thread. It uses
37.3 s of CPU time against the reference's 35.8 s.

n-body's clist variant stores the bodies in a `clist[Body]`, one column per
field, instead of a `list[Body]`, and tracks the cost of indexed clist field
access in loops (issue #759). It runs 129.2 billion instructions against
102.5 billion for the `list[Body]` program and 11.4 billion for the reference.

n-body's unchecked variant is the idiomatic program with the index arithmetic
of `energy` and `advance` in
[`unchecked:` suites](../design/language/03-types/numbers.md#unchecked-blocks),
built with `--unchecked assume`, so it measures what integer overflow checks
cost. List bounds checks remain, since no build setting removes them. It runs
59.9 billion instructions against 102.5 billion for the idiomatic program.

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

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
| `plenty/` | The Plenty programs: the idiomatic one, and any parallel or data-layout variant |

The reference programs are distributed under the Benchmarks Game's revised
BSD license, reproduced in `benchmarks/game/LICENSE-benchmarks-game.txt`.
Each manifest names the program's page and contributors.

| Benchmark | Reference | Threads | Plenty variants | Status |
|---|---|---|---|---|
| [fannkuch-redux](benchmarks-game/fannkuch-redux.md) | C #1 | 1 | idiomatic; parallel (`examples/fannkuch_redux.plenty`, 8 workers) | ready |
| [n-body](benchmarks-game/n-body.md) | Rust #3 | 1 | idiomatic; clist (bodies in a `clist`) | ready |
| [spectral-norm](benchmarks-game/spectral-norm.md) | C #8 | 1 | idiomatic | ready |
| [mandelbrot](benchmarks-game/mandelbrot.md) | C #8 | 1 | idiomatic | ready |
| [fasta](benchmarks-game/fasta.md) | Rust #3 | 1 | idiomatic | ready |
| [k-nucleotide](benchmarks-game/k-nucleotide.md) | Rust #1 | 7 | idiomatic (1 thread); parallel (7 pool workers) | ready |
| [reverse-complement](benchmarks-game/reverse-complement.md) | C #5 | 1 | idiomatic | ready |
| [binary-trees](benchmarks-game/binary-trees.md) | C #1 | 1 | idiomatic | ready |
| [pidigits](benchmarks-game/pidigits.md) | C #1 (GMP) | 1 | none | blocked: no arbitrary-precision integers (backlog B38, issue #481) |
| [regex-redux](benchmarks-game/regex-redux.md) | Rust #1 | 9 | idiomatic, with `std.regex` (1 thread) | ready |

Each reference is one C or Rust program from the site. None is marked with an
asterisk, the site's mark for a program that uses a different algorithm, so
each benchmark has one idiomatic Plenty program that follows the reference's
algorithm. Where the reference uses
threads and the Plenty program does not, the table says so; compare CPU time
(`user_s`) as well as wall time for those.

Each benchmark name links to a page with the complete reference and all available
Plenty variants. Listings include the live source files at documentation build
time, so source changes appear without maintaining a second copy. Plenty blocks
use `plenty fragment` to keep them out of the executable documentation tests;
the [benchmark tests](#tests) check the actual programs.

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
1 GB). `benchmarks/game/baseline.json` holds the full run measured on
2026-11-11 at Plenty 0.8.25, commit `1ade579c`, on the reference laptop
(AMD Ryzen 7 7840HS, performance governor). References were rebuilt and rerun
in the same session with the manifest's native-target flags. Each timing is
the median of three runs pinned to cores `0,2,4,6,8,10,12,14`.

| Benchmark | Size | Reference | Plenty | Ratio |
|---|---:|---:|---:|---:|
| [binary-trees](benchmarks-game/binary-trees.md) | 21 | 10.96 s | 20.10 s | 1.83 |
| [fannkuch-redux](benchmarks-game/fannkuch-redux.md) | 12 | 25.40 s | 24.25 s | 0.95 |
| [fannkuch-redux (parallel, 8 threads)](benchmarks-game/fannkuch-redux.md) | 12 | 25.40 s | 2.77 s | 0.11 |
| [fasta](benchmarks-game/fasta.md) | 25,000,000 | 1.33 s | 1.63 s | 1.22 |
| [k-nucleotide](benchmarks-game/k-nucleotide.md) | 25,000,000 | 11.42 s (7 threads) | 13.87 s | 1.21 |
| [k-nucleotide (parallel, 7 threads)](benchmarks-game/k-nucleotide.md) | 25,000,000 | 11.42 s (7 threads) | 6.74 s | 0.59 |
| [mandelbrot](benchmarks-game/mandelbrot.md) | 16,000 | 11.80 s | 11.92 s | 1.01 |
| [n-body](benchmarks-game/n-body.md) | 50,000,000 | 1.30 s | 2.60 s | 2.01 |
| [n-body (clist)](benchmarks-game/n-body.md) | 50,000,000 | 1.30 s | 4.07 s | 3.14 |
| [pidigits](benchmarks-game/pidigits.md) | 10,000 | blocked | blocked | |
| [regex-redux](benchmarks-game/regex-redux.md) | 5,000,000 | 0.74 s (9 threads) | 3.40 s | 4.60 |
| [reverse-complement](benchmarks-game/reverse-complement.md) | 25,000,000 | 0.39 s | 0.41 s | 1.06 |
| [spectral-norm](benchmarks-game/spectral-norm.md) | 5,500 | 1.18 s | 1.20 s | 1.01 |

Times are median wall-clock seconds; ratios use the unrounded medians. The
JSON retains all three samples, CPU time, peak RSS, instruction/cycle counts,
build commands, tool versions and machine details. Every measured Plenty
variant passed both the small expected-output check and comparison with its
reference at the full size. pidigits remains blocked.

Instruction counts are recorded separately from the three timing repetitions;
use them to compare generated code across sessions.

The [guarded numerical-state optimization](../completed-work/performance.md#guarded-numerical-state-issue-930)
applies to the unchanged original n-body source. At the table's 50 million
steps it now runs 32.1 billion instructions against native Rust's 11.5 billion,
with a 2.01x wall-time ratio. The earlier one-million-step investigation remains
a separate measurement with its own C/Rust comparisons and instruction ledger.

The k-nucleotide and regex-redux references are multithreaded while the
idiomatic Plenty programs run one thread; `user_s` in the JSON gives CPU time
(37.05 s for the k-nucleotide reference, 0.63 s for regex-redux).

k-nucleotide's parallel variant counts the seven frame lengths on seven pool
workers, as the reference does, and is the suite's threaded workload for
reference-count changes (issue #751): each job owns a `Shared` handle to the
sequence, and its dictionary, with heap-string keys for the 12- and 18-byte
frames, returns through a future and is released on the main thread. It uses
19.88 s of CPU time against the reference's 37.05 s. Its three wall-time
samples were 6.74 s, 6.74 s and 4.82 s; the table retains their median.
Use instruction counts for comparisons across sessions on a shared machine.

n-body's clist variant stores the bodies in a `clist[Body]`, one column per
field, instead of a `list[Body]`, and tracks the cost of indexed clist field
access in loops (issue #759). It runs 78.9 billion instructions against
32.1 billion for the `list[Body]` program and 11.5 billion for the reference.
These variants do not currently receive equivalent compiler optimizations.

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
4. Add a source page under `book/src/development/benchmarks-game/`, link it
   from `SUMMARY.md` and the tables, and include each source with mdBook's
   `include` directive. Wrap Plenty includes in `plenty fragment` fences.
5. Run `bench.py check`, then a full run, and update `baseline.json`.

## Reference license

The reference listings retain their source headers and are distributed under
the Benchmarks Game revised BSD license:

```text
{{#include ../../../benchmarks/game/LICENSE-benchmarks-game.txt}}
```

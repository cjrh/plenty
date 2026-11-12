# k-nucleotide

Count nucleotide frequencies and selected sequences in FASTA input.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/knucleotide.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## Rust reference

[Benchmarks Game Rust #1](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/knucleotide-rust-1.html), by Renat Galimov.
Threads: 7. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/k-nucleotide/reference/knucleotide.rs`

```rust
{{#include ../../../../benchmarks/game/k-nucleotide/reference/knucleotide.rs}}
```

The site links futures and indexmap, but this program uses only std, so the local build omits them. The reference counts the 7 frame lengths on 7 threads; the idiomatic Plenty program counts them in turn, and the parallel variant counts them on 7 pool workers.

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/k-nucleotide/plenty/knucleotide.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/k-nucleotide/plenty/knucleotide.plenty}}
```

## Plenty: parallel

Threads: 7.

7 pool workers, one per frame length; each job owns a Shared[Bytes] handle to the sequence and returns its dictionary through a future.

`benchmarks/game/k-nucleotide/plenty/knucleotide_parallel.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/k-nucleotide/plenty/knucleotide_parallel.plenty}}
```

# fasta

Generate repeated and pseudorandom DNA sequences in FASTA format.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/fasta.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## Rust reference

[Benchmarks Game Rust #3](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/fasta-rust-3.html), by the Rust Project Developers; TeXitoi; Matt Brubeck.
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/fasta/reference/fasta.rs`

```rust
{{#include ../../../../benchmarks/game/fasta/reference/fasta.rs}}
```

The site links num_cpus, but this program does not use it, so the local build omits it.

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/fasta/plenty/fasta.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/fasta/plenty/fasta.plenty}}
```

# n-body

Simulate the Jovian planets and report the energy before and after advancing the system.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/nbody.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## Rust reference

[Benchmarks Game Rust #3](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/nbody-rust-3.html), by Ilia Schelokov.
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/n-body/reference/nbody.rs`

```rust
{{#include ../../../../benchmarks/game/n-body/reference/nbody.rs}}
```

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/n-body/plenty/nbody.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/n-body/plenty/nbody.plenty}}
```

## Plenty: clist

Threads: 1.

Bodies in a clist[Body]: each field is its own column. Tracks indexed clist field access in loops (issue #759).

`benchmarks/game/n-body/plenty/nbody_clist.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/n-body/plenty/nbody_clist.plenty}}
```

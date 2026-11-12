# spectral-norm

Approximate the spectral norm of an implicitly defined matrix with the power method.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/spectralnorm.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## C reference

[Benchmarks Game C #8](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/spectralnorm-clang-8.html), by Sebastien Loisel.
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/spectral-norm/reference/spectralnorm.clang-8.c`

```c
{{#include ../../../../benchmarks/game/spectral-norm/reference/spectralnorm.clang-8.c}}
```

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/spectral-norm/plenty/spectralnorm.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/spectral-norm/plenty/spectralnorm.plenty}}
```

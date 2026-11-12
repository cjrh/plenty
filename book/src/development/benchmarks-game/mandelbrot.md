# mandelbrot

Render the Mandelbrot set as a binary PBM image.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/mandelbrot.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## C reference

[Benchmarks Game C #8](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/mandelbrot-clang-8.html), by Greg Buchholz.
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/mandelbrot/reference/mandelbrot.clang-8.c`

```c
{{#include ../../../../benchmarks/game/mandelbrot/reference/mandelbrot.clang-8.c}}
```

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/mandelbrot/plenty/mandelbrot.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/mandelbrot/plenty/mandelbrot.plenty}}
```

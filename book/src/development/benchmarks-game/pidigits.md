# pidigits

Generate digits of pi using arbitrary-precision integer arithmetic.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/pidigits.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## C reference

[Benchmarks Game C #1](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/pidigits-clang-1.html), by Mr Ledrug.
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/pidigits/reference/pidigits.clang-1.c`

```c
{{#include ../../../../benchmarks/game/pidigits/reference/pidigits.clang-1.c}}
```

## Plenty implementation

The single-threaded spigot uses [`std.bigint`](../../design/stdlib/bigint.md)
for fallible arbitrary-precision arithmetic. It follows the bundled C
reference's algorithm and emits ten digits per labelled line; a partial final
line is padded to ten characters. The Plenty program needs no GMP dependency.

```plenty fragment
{{#include ../../../../benchmarks/game/pidigits/plenty/pidigits.plenty}}
```

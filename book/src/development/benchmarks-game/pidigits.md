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

No Plenty program is available yet. Plenty's standard library has no arbitrary-precision integer type; the benchmark needs bignum multiply, add and divide. The reference needs GMP (gmp.h and -lgmp).
See [blocked benchmarks](../benchmarks-game.md#blocked-benchmarks) for backlog B38 and issue #481.

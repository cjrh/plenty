# fannkuch-redux

Enumerate permutations and report the checksum and maximum prefix-reversal count.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/fannkuchredux.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## C reference

[Benchmarks Game C #1](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/fannkuchredux-clang-1.html), by Joseph Piché (C conversion); Oleg Mazurov and Isaac Gouy (Java original).
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/fannkuch-redux/reference/fannkuchredux.clang-1.c`

```c
{{#include ../../../../benchmarks/game/fannkuch-redux/reference/fannkuchredux.clang-1.c}}
```

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/fannkuch-redux/plenty/fannkuchredux.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/fannkuch-redux/plenty/fannkuchredux.plenty}}
```

## Plenty: parallel

Threads: 8.

The existing example: 8 pool workers over blocks of permutations.

`examples/fannkuch_redux.plenty`

```plenty fragment
{{#include ../../../../examples/fannkuch_redux.plenty}}
```

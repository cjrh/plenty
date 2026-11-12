# binary-trees

Allocate, traverse and release perfect binary trees at several depths.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/binarytrees.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## C reference

[Benchmarks Game C #1](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/binarytrees-clang-1.html), by Kevin Carson.
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/binary-trees/reference/binarytrees.clang-1.c`

```c
{{#include ../../../../benchmarks/game/binary-trees/reference/binarytrees.clang-1.c}}
```

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/binary-trees/plenty/binarytrees.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/binary-trees/plenty/binarytrees.plenty}}
```

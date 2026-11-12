# reverse-complement

Read FASTA input and write the reverse complement of each sequence.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/revcomp.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## C reference

[Benchmarks Game C #5](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/revcomp-clang-5.html), by Mr Ledrug; Attila Balazs.
Threads: 1. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/reverse-complement/reference/revcomp.clang-5.c`

```c
{{#include ../../../../benchmarks/game/reverse-complement/reference/revcomp.clang-5.c}}
```

Full mode uses the 25,000,000 fasta input shared with k-nucleotide (about 250 MB) instead of the site's 100,000,000 (about 1 GB), to bound disk use.

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/reverse-complement/plenty/revcomp.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/reverse-complement/plenty/revcomp.plenty}}
```

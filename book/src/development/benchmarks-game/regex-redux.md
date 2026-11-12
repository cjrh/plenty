# regex-redux

Count DNA patterns and perform substitutions with regular expressions.

[Suite, running instructions and baseline](../benchmarks-game.md) · [Benchmark description](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/regexredux.html)

The listings embed the repository sources at documentation build time.
Plenty listings use `plenty fragment` fences; the benchmark suite checks these
programs separately from the executable documentation examples.

## Rust reference

[Benchmarks Game Rust #1](https://benchmarksgame-team.pages.debian.net/benchmarksgame/program/regexredux-rust-1.html), by the Rust Project Developers; BurntSushi; TeXitoi; Matt Brubeck.
Threads: 9. License: [Benchmarks Game revised BSD](../benchmarks-game.md#reference-license).

`benchmarks/game/regex-redux/reference/regexredux.rs`

```rust
{{#include ../../../../benchmarks/game/regex-redux/reference/regexredux.rs}}
```

The site links the regex crate from /opt/src/rust-libs; reference/Cargo.toml pins regex 1.12.3, the version in the workspace lockfile. The reference counts the 9 variants on 9 threads; the idiomatic Plenty program uses std.regex on one thread. The parallel variant uses nine pool workers, with one pattern per job, while the main thread performs the five dependent substitutions. Each job owns its compiled regex and an immutable sequence handle; the sequence bytes are shared without copying. Results are printed in pattern order, and worker and submission errors propagate to the caller.

## Plenty: idiomatic

Threads: 1.

`benchmarks/game/regex-redux/plenty/regexredux.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/regex-redux/plenty/regexredux.plenty}}
```

## Plenty: parallel

Threads: 9 pool workers, plus the main thread, matching the reference's counting parallelism.

`benchmarks/game/regex-redux/plenty/regexredux_parallel.plenty`

```plenty fragment
{{#include ../../../../benchmarks/game/regex-redux/plenty/regexredux_parallel.plenty}}
```

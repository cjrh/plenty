# Compiler commands

These commands compile single files. `plentyc` accepts exactly these commands;
`plenty` accepts them too, alongside the [project commands](packages.md).

| Command | Behavior |
|---|---|
| `plenty FILE` | Compile in a private temporary directory, run, then remove it |
| `plenty --check FILE` | Check an application without emitting or executing code |
| `plenty --check-module FILE` | Check a module and imports without requiring `main` |
| `plenty --compile FILE -o OUT` | Produce a standalone executable |
| `plenty --emit-object FILE -o OUT` | Emit one application object, including imported modules; requires `main` |
| `plenty --fuzz FILE -o OUT` | Produce a coverage-instrumented [libFuzzer executable](fuzzing.md) that runs `main` once per input; `--libfuzzer PATH` selects libFuzzer |
| `plenty --shared-library FILE --library-name NAME -o OUT` | Build a shared C library with generated header and interface |
| `plenty --static-library FILE --library-name NAME -o OUT` | Build a static C library with generated header and interface |
| `plenty --extract-interface LIBRARY --library-name NAME -o OUT` | Extract embedded source metadata without executing native code |
| `plenty --verify-interface LIBRARY INTERFACE` | Verify a generated contract and compatibility symbol |
| `plenty --runtime-interface FILE --library-name NAME -o OUT.plentyi` | Generate a typed runtime loader from checked export source, without loading a library or invoking native tools |
| `plenty --emit-runtime DIR` | Extract `libplenty_runtime.a`, `link-args.txt`, and `target.txt` |
| `plenty --print-target` | Print the packaged runtime's target and CPU baseline |
| `plenty --version` (`-V`) | Print the compiler's version and target, such as `plenty 0.1.0 (x86_64-unknown-linux-gnu)` |

`--release` before `FILE`, `--compile`, or `--fuzz` selects Cranelift's speed
optimizations, like the [release profile](packages.md#profiles).

Compiled functions are reused from the
[compiled-function cache](../02-architecture/cache.md). `--no-cache` compiles
without it, `--cache-stats` reports its hits and misses after compiling, and
`plenty cache stats` and `plenty cache clean` inspect and empty it.

With no arguments, `plenty` displays help. `--module-root DIR` selects the source
root for file commands. `--package-graph GRAPH` instead resolves imports through
a [package graph file](packages.md#package-graph-files) for `FILE`, `--check`,
`--check-module`, and `--compile`. Library builds do not require `main`; their exported
contracts follow the [C interface rules](../../facilities/04-interop/index.md).

Compile-and-run inherits standard streams, environment, and working directory.
It propagates the child's exit code; Unix signal termination becomes 128 plus
the signal number. Temporary objects and runtime files are removed on success
and failure. Compile-and-run startup time includes compilation and linking.

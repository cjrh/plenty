# Compiler commands

These commands compile single files. `plentyc` accepts exactly these commands;
`plenty` accepts them too, alongside the [project commands](packages.md).

| Command | Behavior |
|---|---|
| `plenty FILE` | Compile in a private temporary directory, run, then remove it |
| `plenty --check FILE` | Check an application without emitting or executing code |
| `plenty --check-module FILE` | Check a module and imports without requiring `main` |
| `plenty --compile FILE -o OUT` | Produce a native executable (dynamic dependencies remain) |
| `plenty --compile FILE --dist DIR` | Publish a new [application distribution](distributions.md) with only reached engines and their notices |
| `plenty --emit-object FILE -o OUT` | Emit one application object, including imported modules; requires `main` |
| `plenty --dump STAGES FILE -o DIR` | Write each function's [IR and machine code as text](../02-architecture/dump.md); `--dump-std` and `--dump-filter TEXT` select functions |
| `plenty --fuzz FILE -o OUT` | Produce a coverage-instrumented [libFuzzer executable](fuzzing.md) that runs `main` once per input; `--libfuzzer PATH` selects libFuzzer |
| `plenty --shared-library FILE --library-name NAME -o OUT` | Build a shared C library with generated header and interface |
| `plenty --static-library FILE --library-name NAME -o OUT` | Build a static C library with generated header and interface |
| `plenty --extract-interface LIBRARY --library-name NAME -o OUT` | Extract embedded source metadata without executing native code |
| `plenty --verify-interface LIBRARY INTERFACE` | Verify a generated contract and compatibility symbol |
| `plenty --runtime-interface FILE --library-name NAME -o OUT.plentyi` | Generate a typed runtime loader from checked export source, without loading a library or invoking native tools |
| `plenty --emit-runtime DIR` | Extract `libplenty_runtime.a`, `link-args.txt`, and `target.txt` |
| `plenty --print-target` | Print the packaged runtime's target and CPU baseline |
| `plenty --version` (`-V`) | Print the compiler's version and target: `plenty X.Y.Z (x86_64-unknown-linux-gnu)`, where `X.Y.Z` is the installed version |

`--check-all-methods` with `--check` or `--check-module` also checks every
method of every instance the program creates of a generic class or enum it
declares, including methods nothing uses, which are otherwise
[checked only when used](../../language/07-generics/index.md#checking-every-method).
Types from `std` keep per-use checking.
Other modes reject it: compiling would emit those methods, and a reused cached
object would skip their check.

`--release` before `FILE`, `--compile`, `--fuzz`, or `--dump` selects Cranelift's speed
optimizations, like the [release profile](packages.md#profiles).

`--test-hooks` with `FILE` or `--compile` enables
[failure injection](../../stdlib/test.md#failure-injection) in an ordinary
executable. It does not arm failures automatically. Default builds have no hooks;
library, object-only, fuzz, and checking modes reject this flag.

`--compile` and `--shared-library` output is
[stripped](linking.md#symbols-and-debug-information) of its symbol table and
debug sections; `--no-strip` keeps them.

Compiled functions are reused from the
[compile cache](../02-architecture/cache.md), and so is a whole program's
object when none of its sources changed. `--no-cache` compiles without it,
`--no-program-cache` reuses functions but not programs, `--cache-stats`
reports its hits and misses after compiling, and `plenty cache stats` and
`plenty cache clean` inspect and empty it.

With no arguments, `plenty` displays help. `--module-root DIR` selects the source
root for file commands. `--package-graph GRAPH` instead resolves imports through
a [package graph file](packages.md#package-graph-files) for `FILE`, `--check`,
`--check-module`, and `--compile`. Library builds do not require `main`; their exported
contracts follow the [C interface rules](../../facilities/04-interop/index.md).

Compile-and-run inherits standard streams, environment, and working directory.
It propagates the child's exit code; Unix signal termination becomes 128 plus
the signal number. Temporary objects and runtime files are removed on success
and failure. Compile-and-run startup time includes compilation and linking.

The proposed [`plenty script` workflow](../../../proposals/single-file-scripts.md)
would add embedded manifests, script arguments and reusable executables. It is
not implemented; the commands above describe current behavior.

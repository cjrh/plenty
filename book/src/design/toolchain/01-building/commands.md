# Compiler commands

Source paths accept `.plenty` or `.p`; interface paths accept `.plentyi` or
`.pi`. Generated library companions keep the `.plentyi` spelling by default;
`--runtime-interface` can explicitly write either interface extension.


These commands compile single files. `plentyc` accepts exactly these commands;
`plenty` accepts them too, alongside the [project commands](packages.md).

| Command | Behavior |
|---|---|
| `plenty FILE` | Reuse a cached executable or compile/link, run privately, then remove the temporary directory |
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

Checking modes print [warnings](warnings.md) on stderr; they do not change the
exit status.

`--check-all-methods` with `--check` or `--check-module` also checks every
method of every instance the program creates of a generic class or enum it
declares, including methods nothing uses, which are otherwise
[checked only when used](../../language/07-generics/index.md#checking-every-method).
Types from `std` keep per-use checking.
Other modes reject it: compiling would emit those methods, and a reused cached
object would skip their check.

Code generation is optimized by default, including `plentyc`, object and shared
library output, fuzz targets and dumps. `--fast-compile` skips speed optimizations
for shorter edit-run cycles in large projects; it preserves all language checks.
`--release` remains a compatibility alias for the optimized default. See
[profiles](packages.md#profiles).

`--cpu LEVEL` selects the [CPU level](linking.md#cpu-levels) of an executable:
`x86-64` (default), `x86-64-v2` or `x86-64-v3`. Above `x86-64` the executable
checks the CPU at startup and exits with an error on one without the level.
Shared and static libraries reject levels above `x86-64`.

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
`--no-program-cache` reuses functions but not programs or executables, `--cache-stats`
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
and failure. On supported Linux toolchains, single-file runs also reuse a
[cached executable](../02-architecture/cache.md#single-file-executables) by
default, avoiding linking when its inputs match. Other runs link normally.
Unix scripts can use `#!/usr/bin/env plenty` as their first line and be made
executable with `chmod +x FILE`; hashbang launches use the same caches.

For argument forwarding, optional embedded manifests and isolated dependency
resolution, use [`plenty script`](scripts.md), with the Unix shebang
`#!/usr/bin/env -S plenty script`. The compiler shortcuts above remain unchanged.

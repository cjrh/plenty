# Native linking and targets

Native emission supports **`x86_64-unknown-linux-gnu` only**. `--target TRIPLE`
requires an exact match between the requested target, packaged runtime, and
Cranelift ISA before emission. Unsupported explicit targets fail before source
loading. This flag does not enable cross-compilation. Checking is independent of
native emission unless a target is explicitly requested.

`--linker PATH` selects a `cc`-compatible driver; the default is `cc` on `PATH`.
Repeat `--link-arg ARG` to pass individual arguments after program/runtime inputs
and before the runtime's native dependencies. Static-library builds use
`--archiver PATH`, defaulting to `ar`; it must support deterministic `rcsD` mode.

With no `--linker` or `--link-arg` override, Plenty prefers **mold**, then **lld**,
then the driver's default linker. It looks for executable `ld.mold` or `ld.lld`
on `PATH` and checks that `cc -fuse-ld=NAME -Wl,--version` succeeds before
selecting one. An unavailable or rejected candidate falls through to the next
choice. A failure linking the actual program is reported without retrying with
a different linker.

Discovery runs once per compiler process; set `PATH` and the driver's
environment before starting it. The probe's result is also recorded in the
[compile cache](../02-architecture/cache.md) directory, under
`DIR/linker-probe/`, so later processes skip the probe. A record's key covers
`PATH`, `COMPILER_PATH`, and the resolved path, size and modification time of
`cc` and of each `ld.mold` and `ld.lld` found on `PATH`: installing, removing,
upgrading or touching any of them causes a fresh probe. The directory keeps the
64 newest records. With the cache off (`PLENTY_CACHE=off`, `--no-cache`), each
process probes again. A linker that `cc` finds elsewhere than `PATH`, such as
its own installation directory, is not part of the key; after changing one,
remove `DIR/linker-probe/`.

`PLENTY_LINKER_THREADS=N` limits the automatically selected linker to `N`
threads: it adds `-Wl,--thread-count=N` for mold and `-Wl,--threads=N` for
lld. Unset or empty keeps the linker's default, and a value that is not a
positive integer fails the link. It does not apply to a linker selected with
`--link-arg` or found through `COMPILER_PATH`. mold uses every core by default,
which is fastest for one link; when many links run at once, as in a parallel
test run, each link's threads compete with the others, and one thread per link
finished 400 links at 8 at a time in about 60% of the time.

Pass `--verbose` to report the effective driver command. `--linker cc` opts out
of automatic selection and uses the driver's configured default; to pin a
particular linker, use e.g. `--link-arg -fuse-ld=bfd` or
`--link-arg -fuse-ld=mold`. Any explicit `--link-arg`, even `-lm`, disables
automatic selection. These options apply to both compiler binaries, including
shared libraries and fuzz executables. Object emission does not probe a linker.
In the Rust API, `CompileOptions::auto_linker = false` opts out; non-default
drivers and nonempty `link_args` also disable discovery. `verbose` reports the
command. The caller must explicitly disable `auto_linker` when selecting `cc`
itself, since assigning that default value cannot record an explicit override.

## Symbols and debug information

Executables and shared libraries are linked with `-Wl,--strip-all`, which
drops the symbol table (`.symtab`, `.strtab`) and every `.debug_*` section.
Generated code carries no debug information; these sections come from the Rust
runtime archive and its precompiled standard library, and were about three
quarters of a hello-world executable:

| Program | Unstripped | Stripped |
|---|---:|---:|
| empty `main` | 2,727,008 bytes | 302,280 bytes |
| `print("hello")` | 3,289,728 bytes | 789,776 bytes |

Measured with mold. Stripping leaves the loaded segments unchanged (`size`
reports the same `text`, `data` and `bss`), so it affects file size, not
memory use or speed.

Nothing reads these sections at run time. Trap reports find the failed check
through the compiler's site and `ud2` tables, which the linker resolves as
relocations, not through symbols; the runtime prints no backtraces. A shared
library keeps its dynamic symbol table (`.dynsym`), so its exports and
`--verify-interface` are unaffected, and its embedded `.plenty.interface.*`
metadata is not a debug section.

`--no-strip` keeps both, for `--compile` and `--shared-library`; `plenty build`
and `plenty run` accept it too. Use it to inspect an executable with `nm`,
`perf`, or a debugger, or to split the debug information into a separate
file:

```text
plenty --compile --no-strip app.plenty -o app
objcopy --only-keep-debug app app.debug
objcopy --strip-all --add-gnu-debuglink=app.debug app
```

`--fuzz` targets and `plenty test` executables are never stripped: they run
and are debugged where they are built. Static libraries are archives of
unlinked objects, so the final link decides; `--emit-object` likewise leaves
stripping to the external link (see below). In the Rust API,
`CompileOptions::strip` is `true` by default.

Stripping is part of the linker's deterministic output and does not change the
reproducibility contract below; package build fingerprints include it.

## Size budgets

`tests/test_executable_size.rs` fails when a small program grows past its
budget. Measured with the runtime from issues #500, #501 and #503:

| Program | Stripped file | `.text` | Budget (file, `.text`) |
|---|---:|---:|---:|
| empty `main` | 47,648 bytes | 21,153 bytes | 54,000, 24,000 |
| `print("hello")` | 113,544 bytes | 69,137 bytes | 128,000, 78,000 |

The `runtime-checks` test feature links a runtime with allocation accounting,
about 6–9 KB larger; it has its own budgets.

The same file checks, for each `std` module with runtime native code, that
hello links none of that code, that importing any `std` module links none of
it, and that a program calling the module does link it. Its `NATIVE_MODULES`
table names each module, a program, and the symbols that prove the code is
linked: the module's [entry symbols](../02-architecture/runtime.md#entry-symbols)
and, for a vendored library, part of its symbol names. A new native wrapper
adds a row. Raising a budget is a reviewed change that records the new
measurement beside `BUDGETS`.

## Reproducible artifacts

Generated code uses the original x86-64/SSE2 baseline, regardless of the
compiler machine's CPU extensions. `--print-target` reports this baseline.
With the same compiler binary, source tree, module contents, and options,
application objects and extracted runtime artifacts are independent of source
directory, output name, time, user, and code-generation worker count. Static
archives use deterministic member timestamps, owner IDs, and modes.

Executables and shared libraries additionally depend on the linker driver,
its options, startup objects, and native libraries. Automatic selection makes
the available linkers and driver environment inputs too: pin the linker for
reproducible builds across machines. Package build fingerprints include the
automatically selected linker name. Different versions of the same tool still
require a clean rebuild after changing the native toolchain. Determinism is
checked with the locally selected toolchain; other linker implementations are
separate inputs. A program that reaches a [native engine](engines.md) also
depends on the engine's directory, which its `RUNPATH` records.
Rebuilding the compiler also depends on rustc and its precompiled standard
library. Runtime compilation remaps project, output, and sysroot source paths;
it cannot rewrite paths already compiled into the Rust standard library.
Changing the compiler or any of these toolchain inputs can change output bytes.

Paths and arguments are passed directly, without a shell, expansion, or word
splitting. There is no `CC` environment override. The linker driver must accept
object/archive inputs, Unix native flags, and `-o OUT`; GCC and Clang do. Drivers
with different conventions need a wrapper. Choosing a driver does not expand
target support. Failures report its identity, status, and diagnostics.

## External build systems

`--emit-object` requires no linker and exports `plenty_main`. Link exactly one
application object, followed by the runtime archive extracted from the same
compiler build and its native arguments, one per line in `link-args.txt`. The
first argument is `-Wl,--gc-sections`. The object puts each function and data
object in its own section, so the linker drops every function `main` cannot
reach, including the unused functions of imported `std` modules, and then the
runtime code only they call, such as the [`std.crypto`](../../stdlib/crypto.md)
hashes, the [`std.compress`](../../stdlib/compress.md) codecs and the
[`std.tls`](../../stdlib/tls.md) stack. The
runtime supplies C `main`. `link-args.txt` lists only what linking requires,
so it does not strip; add `-Wl,--strip-all` (or `-s`) to match `plenty
--compile`. Neither object emission nor runtime extraction needs
Cargo or rustc on `PATH`.

Ordinary Plenty functions, including `pub` functions, use private symbols and
calling conventions. Application object output does not expose a C library API
or provide separate Plenty module compilation; use a library build for exports.

The Rust API exposes `CompileOptions`, executable compilation with options,
`compile_source_to_object`, `compile_file_to_object`, and `emit_runtime`. Runtime
extraction returns archive and argument paths/values for build-system integration.

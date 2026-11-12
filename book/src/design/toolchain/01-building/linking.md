# Native linking and targets

Native emission implements **`x86_64-unknown-linux-gnu`** and
**`aarch64-unknown-linux-gnu`**. ARM deployment qualification is described in
[Linux AArch64 development](../../../development/linux-aarch64.md). `--target TRIPLE`
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

## CPU levels

`--cpu LEVEL` selects the x86-64 instruction set an executable's generated code
may use. `[build] cpu` in [plenty.toml](packages.md#manifest) sets it for a
package or script; `--cpu` overrides it. The levels are the x86-64 psABI's:

| Level | Code generation may use | Startup check also requires |
|---|---|---|
| `x86-64` (default) | SSE2 | nothing: no check is emitted |
| `x86-64-v2` | SSE3, SSSE3, SSE4.1, SSE4.2, POPCNT, CMPXCHG16B | LAHF/SAHF |
| `x86-64-v3` | v2, AVX, AVX2, FMA, BMI1, BMI2, LZCNT | MOVBE, F16C, OSXSAVE, and XCR0 enabling SSE and AVX state |

The list is closed: there is no `native` level, so an executable's instructions
never depend on the machine that compiled it. An unknown level is an error that
lists the valid ones, and a level other than `x86-64` on an AArch64 target is an
error. The default output is unchanged by this feature.

At `x86-64-v3`, Cranelift encodes SSE operations with VEX, uses three-operand
forms, and selects BMI and LZCNT instructions. Optimized code holds
`std.simd.f64x4` and `mask64x4` in one 256-bit `ymm` register each on Linux and
macOS; other `std.simd` shapes stay 128 bits wide, and Windows keeps `f64x4` as
two 128-bit halves because its calling convention preserves `xmm6`-`xmm15`.
Code generation never contracts `x * y + z` into a fused multiply-add, at any
level, so scalar and `std.simd` float results are bit-identical across levels.

A function that holds a 256-bit value executes `vzeroupper` before every call
and return, so the runtime, C code compiled for SSE and other Plenty functions
never run with dirty upper halves, and no 256-bit value is live across a call:
it is spilled. In such a function every float spill slot and register move is
256 bits wide. Arguments and results are at most 128 bits: calls pass an
`f64x4` as they do at other levels. The 256-bit lowering is
a change in Plenty's [Cranelift fork](../../../development/workflow.md#cranelift-and-regalloc2-forks),
not upstream Cranelift.

**Startup check.** Above `x86-64`, the exported `plenty_main` is a trampoline
compiled at the baseline. It calls the runtime's `plenty_rt_require_cpu`, which
reads CPUID (and XCR0 through `xgetbv` only when OSXSAVE is set), before it calls
the program. The runtime's `main` runs only argument capture and stack-overflow
handler setup, both baseline code, before `plenty_main`; fuzz executables reach
the program through the same entry. A CPU without every required feature makes
the program print one line to stderr and exit with status 1 before any Plenty
code runs; nothing traps or aborts:

```text
error: this program requires an x86-64-v3 CPU (missing: fma, avx, avx2, ...)
```

There is no option to skip the check. `x86-64` executables contain no check and
no trampoline.

**Runtime.** The runtime library is compiled once at the baseline and linked into
programs of every level; it does not use instructions above SSE2.

**Libraries.** `--shared-library` and `--static-library` reject levels above
`x86-64`: a library has no entry point to run the check from, and its exports
can be called directly by a host on any CPU. A per-export check, or a load-time
check, is an open question in issue #962.

**Caches and reproducibility.** The level is part of the
[compile cache](../02-architecture/cache.md) keys of functions, program objects
and executables, and of the [build fingerprint](packages.md#build-reuse), so one
cache serves every level without mixing their code. The same sources, compiler
and level produce the same bytes. `--print-target --cpu LEVEL` prints the
target with the level; `--fast-compile` keeps the level.

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

Measured on x86-64 with mold. Stripping leaves the loaded segments unchanged (`size`
reports the same `text`, `data` and `bss`), so it affects file size, not
memory use or speed.

Nothing reads these sections at run time. Trap reports find the failed check
through the compiler's site and trap-instruction tables, which the linker resolves as
relocations, not through symbols; the runtime prints no backtraces. A shared
library keeps its dynamic symbol table (`.dynsym`), so its exports and
`--verify-interface` are unaffected, and its embedded `.plenty.interface.*`
metadata is not a debug section.

`--no-strip` keeps both, for `--compile` and `--shared-library`; `plenty build`
and `plenty run` accept it too. For single-file compilation with `plenty` or
`plentyc`, `--fast-compile` and `--no-strip` may appear in either order before or
after `--compile FILE`, including between the mode and filename.
Use `--no-strip` to inspect an executable with `nm`,
`perf`, or a debugger, or to split the debug information into a separate
file:

```text
plenty --compile --no-strip app.plenty -o app
objcopy --only-keep-debug app app.debug
objcopy --strip-all --add-gnu-debuglink=app.debug app
```

`--fuzz` targets, `plenty test` executables, and explicit `--test-hooks` builds
are never stripped: they run
and are debugged where they are built. Static libraries are archives of
unlinked objects, so the final link decides; `--emit-object` likewise leaves
stripping to the external link (see below). In the Rust API,
`CompileOptions::strip` is `true` by default.

Stripping is part of the linker's deterministic output and does not change the
reproducibility contract below; package build fingerprints include it.

## Size budgets

`tests/test_executable_size.rs` fails when an empty program or a hello program
grows past its stripped-file or `.text` budget. The measurements and current
limits live beside `BUDGETS` in that file. x86-64 and ARM64 have separate limits,
as do ordinary runtimes and the `runtime-checks` feature's allocation accounting.
Compare both sizes: linker layout and segment alignment can increase the file
without increasing its code. ARM files with a separate read-only LOAD segment
after executable code and 64 KiB segment alignment use a separately measured
file-size budget. Compact layouts keep their smaller file limits, and both
layouts keep the same `.text` limits.

A failure preserves the measured executable, source, and `layout.txt` in the
directory printed by the test. The report records sizes, section offsets,
segment alignment, ELF producer comments, runtime-checks mode, Git revision,
and available toolchain versions. Missing version commands are recorded rather
than preventing artifact retention. Set `PLENTY_SIZE_ARTIFACT_DIR` to a dedicated
directory to retain successful measurements too; each sample gets its own
subdirectory. CI uploads these samples as `executable-size-ARCH-shard-N`
artifacts with seven-day retention, so comparisons can use the actual runner
ELFs. Failed Linux release builds also upload their samples as
`release-size-evidence-TARGET` artifacts with fourteen-day retention. Local
failures and explicitly requested samples remain until removed.
Tool versions queried during the test describe the current environment;
ELF producer comments can identify the actual linker (mold and lld record
their versions there).

The same file checks, for each `std` module with runtime native code, that
hello links none of that code, that importing any `std` module links none of
it, and that a program calling the module does link it. Its `NATIVE_MODULES`
table names each module, a program, and the symbols that prove the code is
linked: the module's [entry symbols](../02-architecture/runtime.md#entry-symbols)
and, for a vendored library, part of its symbol names. A new native wrapper
adds a row. Raising a budget is a reviewed change that records the new
measurement beside `BUDGETS`.

## Reproducible artifacts

Generated code uses the original x86-64/SSE2 baseline or generic ARMv8-A,
regardless of the compiler machine's CPU extensions. ARM does not require
optional LSE, SVE or crypto instructions. `--print-target` reports the native
triple and CPU baseline. Reproducibility applies within one target and fixed
toolchain; artifacts from different architectures have different bytes.
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
depends on the engine's directory, which ordinary output records in `RUNPATH`.
[Distribution output](distributions.md) instead uses only `$ORIGIN` and copies
the selected engines and their notices.
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

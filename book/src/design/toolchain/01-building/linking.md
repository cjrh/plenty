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
selecting one. Discovery is cached for the compiler process; set `PATH` and the
driver's environment before starting it. An unavailable or rejected candidate
falls through to the next choice. A failure linking the actual program is
reported without retrying with a different linker.

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
separate inputs.
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
runtime supplies C `main`. Neither object emission nor runtime extraction needs
Cargo or rustc on `PATH`.

Ordinary Plenty functions, including `pub` functions, use private symbols and
calling conventions. Application object output does not expose a C library API
or provide separate Plenty module compilation; use a library build for exports.

The Rust API exposes `CompileOptions`, executable compilation with options,
`compile_source_to_object`, `compile_file_to_object`, and `emit_runtime`. Runtime
extraction returns archive and argument paths/values for build-system integration.

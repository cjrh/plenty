# Native linking and targets

Native emission supports **`x86_64-unknown-linux-gnu` only**. `--target TRIPLE`
requires an exact match between the requested target, packaged runtime, and
Cranelift ISA before emission. Unsupported explicit targets fail before source
loading. This flag does not enable cross-compilation. Checking is independent of
native emission unless a target is explicitly requested.

`--linker PATH` selects a `cc`-compatible driver; the default is `cc` on `PATH`.
Repeat `--link-arg ARG` to pass individual arguments after program/runtime inputs
and before the runtime's native dependencies. Static-library builds use
`--archiver PATH`, defaulting to `ar`.

Paths and arguments are passed directly, without a shell, expansion, or word
splitting. There is no `CC` environment override. The linker driver must accept
object/archive inputs, Unix native flags, and `-o OUT`; GCC and Clang do. Drivers
with different conventions need a wrapper. Choosing a driver does not expand
target support. Failures report its identity, status, and diagnostics.

## External build systems

`--emit-object` requires no linker and exports `plenty_main`. Link exactly one
application object, followed by the runtime archive extracted from the same
compiler build and its native arguments, one per line in `link-args.txt`. The
runtime supplies C `main`. Neither object emission nor runtime extraction needs
Cargo or rustc on `PATH`.

Ordinary Plenty functions, including `pub` functions, use private symbols and
calling conventions. Application object output does not expose a C library API
or provide separate Plenty module compilation; use a library build for exports.

The Rust API exposes `CompileOptions`, executable compilation with options,
`compile_source_to_object`, `compile_file_to_object`, and `emit_runtime`. Runtime
extraction returns archive and argument paths/values for build-system integration.

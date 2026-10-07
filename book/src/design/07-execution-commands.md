# Execution commands

- `plenty FILE` compiles into a private temporary directory, executes the native
  binary, then removes the directory. The child inherits standard input/output,
  the environment, and the current working directory. Its exit code is propagated;
  on Unix, signal termination is reported as 128 plus the signal number.
- `plenty --compile FILE -o OUT` produces a standalone executable.
- `plenty --shared-library FILE --library-name NAME -o OUT` and
  `--static-library` produce [C libraries](24-c-interfaces/02-library-exports.md)
  with generated headers/interfaces, without requiring `main`. Static output
  uses `--archiver PATH` (default `ar`); shared output uses `--linker`.
- `plenty --extract-interface LIBRARY --library-name NAME -o OUT` extracts
  embedded source metadata without executing native code.
- `plenty --runtime-interface FILE --library-name NAME -o OUT.plentyi` generates
  a [typed runtime loader](24-c-interfaces/06-runtime-loading.md) from checked
  export source. It uses no native toolchain and does not load the library.
- `plenty --verify-interface LIBRARY INTERFACE` checks a generated contract
  and its compatibility symbol before linking; see [inspection](24-c-interfaces/04-embedded-contracts.md).
- `plenty --emit-object FILE -o OUT` emits one native application object,
  including imported modules, without invoking a linker. It still requires `main`.
- `plenty --emit-runtime DIR` extracts the embedded `libplenty_runtime.a` and
  `link-args.txt`, with one required native driver argument per line, plus `target.txt`.
- `plenty --print-target` prints the target of the packaged runtime.
- `--target TRIPLE` explicitly requires that target. Only native
  `x86_64-unknown-linux-gnu` is currently supported; other triples fail before
  source loading/code generation. This option does not enable cross-compilation.
- `plenty --check FILE` validates without native emission, linking, or execution.
- `plenty --check-module FILE` checks a library and its imports without requiring `main`.
- `--module-root DIR` selects the source root for modern file commands.
- `--linker PATH` selects a `cc`-compatible linker driver for compilation or
  execution. The default is `cc` on `PATH`.
- Repeat `--link-arg ARG` to pass individual native driver arguments verbatim,
  after the program and runtime inputs and before the runtime's native libraries.
- `plenty` with no arguments displays help.

Both execution commands use the same compiler and embedded runtime. Running
and compiling require a linker driver; checking does not. Neither
command invokes Cargo or rustc, and a relocated compiler binary needs no runtime
source files.
Temporary object/runtime files are also removed after success or failure.
Compile-and-run adds compilation and linking to startup time; benchmark both
as part of the edit/run workflow.

The Rust API's `CompileOptions` and `compile_*_to_executable_with_options`
functions expose the same configuration. Paths and arguments are passed directly,
without a shell, environment-variable expansion, or word splitting. No `CC`
environment override is implied. The driver must accept object/archive inputs,
Unix native link flags, and `-o OUT`, as GCC and Clang do. Raw `ld`, MSVC's
`link.exe`, and drivers with different conventions need a wrapper; selecting a
driver does not enable another target. Failures identify the driver and report
its status and diagnostics.

Object emission exports `plenty_main`, which the runtime's C `main` calls. Link
exactly one Plenty application object, followed by the runtime archive from the
same compiler build and its native dependencies. Ordinary Plenty functions,
including `pub` functions, use private symbols and calling conventions. Object
output does not yet export a C library API or enable separate Plenty module
compilation. The Rust API exposes `compile_source_to_object`,
`compile_file_to_object`, and `emit_runtime`; the latter also returns archive and
argument paths/values for a build system. Object emission and runtime extraction
need no linker, Cargo, or rustc on PATH.

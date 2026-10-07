# Execution commands

- `plenty FILE` compiles into a private temporary directory, executes the native
  binary, then removes the directory. The child inherits standard input/output,
  the environment, and the current working directory. Its exit code is propagated;
  on Unix, signal termination is reported as 128 plus the signal number.
- `plenty --compile FILE -o OUT` produces a standalone executable.
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

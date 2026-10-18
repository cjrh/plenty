# Building and running programs

Plenty compiles ahead of time through Cranelift. Checking needs no native linker;
running and executable builds use a `cc`-compatible driver and the runtime archive
embedded in the compiler. Program builds invoke neither Cargo nor rustc.

- [Installing a release](installing.md): release archives, requirements, and
  `PATH` setup.
- [Commands](commands.md): checking, running, executable and library output.
- [Neovim](neovim.md): file detection, syntax highlighting, and editing defaults.
- [Packages](packages.md): `plenty.toml`, path dependencies, `plenty.lock`,
  profiles, and the `plenty build`, `run`, `check`, and `test` commands.
- [Application exits](entry-report.md): statuses and returned-error reports.
- [Native linking and targets](linking.md): driver arguments, object output,
  runtime extraction, and supported platforms.
- [Library artifacts](artifacts.md): generated files, contract inspection, and
  publication guarantees.
- [Fuzzing](fuzzing.md): coverage-instrumented libFuzzer builds with `--fuzz`.

For source-level entrypoint rules, see [source and modules](../../language/02-source/index.md).
For runtime packaging, see [architecture](../02-architecture/runtime.md).

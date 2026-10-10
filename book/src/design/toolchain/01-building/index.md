# Building and running programs

Plenty compiles ahead of time through Cranelift. Checking needs no native linker;
running and executable builds use a `cc`-compatible driver and the runtime archive
embedded in the compiler. Program builds invoke neither Cargo nor rustc.

- [Commands](commands.md): checking, running, executable and library output.
- [Application exits](entry-report.md): statuses and returned-error reports.
- [Native linking and targets](linking.md): driver arguments, object output,
  runtime extraction, and supported platforms.
- [Library artifacts](artifacts.md): generated files, contract inspection, and
  publication guarantees.

For source-level entrypoint rules, see [source and modules](../../language/02-source/index.md).
For runtime packaging, see [architecture](../02-architecture/runtime.md).

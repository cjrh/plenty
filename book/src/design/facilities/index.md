# Built-in facilities and interoperability

Operation contracts state what they consume or borrow, whether they allocate or
block, and what remains after failure.

- [Built-in types and operations](01-builtins/index.md): numbers, text, tuples,
  lists, dictionaries, sets, and ranges.
- [Standard math library](math.md): embedded Plenty algorithms and recoverable
  domain/overflow errors.
- [Input, output, and files](02-io/index.md): arguments, streams, whole-file
  helpers, and owned file handles.
- [Concurrency and parallelism](03-concurrency/index.md): certified workers,
  ownership transfer, channels, pools, cancellation, and bounded waits.
- [C interoperability](04-interop/index.md): trusted imports, text and ownership
  adapters, library exports, metadata, and runtime loading.

These APIs follow the [ownership](../language/05-ownership/index.md) and
[error/allocation](../language/08-errors/index.md) rules.

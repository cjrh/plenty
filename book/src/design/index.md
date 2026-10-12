# Reference

The reference describes implemented behavior and current restrictions. Start with
the [language overview](language/01-overview/index.md) for the connecting ideas,
or use the [support index](toolchain/02-architecture/support.md) to find a contract.

- **Language contract:** [source and modules](language/02-source/index.md),
  [types](language/03-types/index.md), [expressions](language/04-expressions/index.md),
  [ownership](language/05-ownership/index.md), [functions](language/06-functions/index.md),
  [generics](language/07-generics/index.md), [errors and resource scopes](language/08-errors/index.md),
  and [iteration](language/09-iteration/index.md) define what programs mean.
- **Built-in facilities and interoperability:**
  [built-in operations](facilities/01-builtins/index.md), [I/O](facilities/02-io/index.md),
  [concurrency](facilities/03-concurrency/index.md), and
  [C interfaces](facilities/04-interop/index.md) specify available operations,
  ownership transfer, allocation, and failure behavior.
- **Standard library:** [imported `std` modules](stdlib/index.md), currently
  [`std.math`](stdlib/math.md), are written in Plenty and embedded in the
  compiler.
- **Toolchain and implementation:** [building programs](toolchain/01-building/index.md)
  explains commands and artifacts; [architecture](toolchain/02-architecture/index.md)
  explains checking, native compilation, and runtime mechanisms.

[Learn Plenty](../tutorial/index.md) teaches the language through executable
examples. [Supplementary examples](examples/index.md) cover additional operations.
Design alternatives live in [Proposals](../proposals/index.md); future work lives
in the [backlog](../backlog.md).

# Language overview

Plenty is a statically typed, ahead-of-time compiled language with
Python-shaped syntax and Rust-influenced behaviours. Its design favors fast
compilation, explicit interfaces, and predictable memory use.

Every function declares its parameter and return types. Local expressions can
infer types, while [generics](../07-generics/index.md) reuse definitions at concrete
types. Classes and enums have fixed declarations rather than dynamic attributes
or inheritance.

[Ownership](../05-ownership/index.md) makes copies, moves, shared access, and
exclusive mutation explicit. Owned resources have deterministic cleanup.
[Recoverable errors](../08-errors/index.md) are values; operations that allocate
can report allocation failure without exceptions.

The language contract describes [source organization](../02-source/index.md),
[types](../03-types/index.md), [expressions](../04-expressions/index.md),
[callable values](../06-functions/index.md), and [iteration](../09-iteration/index.md).
Built-in operation contracts belong to [facilities](../../facilities/index.md);
building and executing programs belongs to the [toolchain](../../toolchain/index.md).

# Learning Plenty

Plenty combines Python-shaped syntax with explicit types, ownership, and native
compilation. This guide teaches the language that works today. Each program is
self-contained, and its output or expected diagnostic is checked by
`cargo test --test test_tutorial`.

Start with the eight core parts in order. They introduce concepts when a program
first needs them, from printing a value to building a small command-line tool.

1. [Your first program](01-first-program/index.md): compile, run, and check.
2. [Values, functions, and control flow](02-values/index.md): name values,
   choose types, define functions, and repeat work.
3. [Absence and failure](03-absence/index.md): handle missing values and errors
   with `Option`, `Result`, `match`, and `?`.
4. [Collections, ownership, and borrowing](04-collections/index.md): keep
   groups of values, transfer ownership, copy, and borrow.
5. [Iteration and transformation](05-iteration/index.md): visit values,
   transform them, and produce them incrementally.
6. [Defining your own data](06-data/index.md): write records, methods,
   constructors, and enums.
7. [Organizing a program](07-modules/index.md): split a program into modules
   and choose its public interfaces.
8. [Useful programs and resources](08-resources/index.md): use command-line
   arguments, text, standard streams, and files in a complete application.

After the core, you can write useful sequential programs. Choose an optional
path when your own program needs it:

- [Generic code and protocols](09-generics/index.md) reuse algorithms across
  different types.
- [Functions as values](10-functions/index.md) introduce callbacks and captured
  state, then stored and consuming callbacks.
- [Concurrent and parallel programs](11-concurrency/index.md) coordinate
  workers, communicate, stop work, and bound waiting.
- [Working with C libraries](12-c-libraries/index.md) call foreign code, own
  foreign resources, and export library interfaces. This path does not require
  concurrency.

The [Reference](../design/index.md) describes detailed contracts and limits.
Supplementary reference examples cover method catalogues and advanced file
operations, so you can look them up without interrupting the core path.
The final [next steps](next-steps.md) page points toward further exploration.

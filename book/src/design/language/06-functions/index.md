# Functions and callable values

Functions declare complete signatures. Named functions and capture-free
anonymous functions have `Callable` types; explicit captures create concrete
closure environments. Reusable calls borrow an environment, while consuming
calls transfer it into the body.

- [Signatures, methods, and calls](01-signatures-and-calls.md)
- [Named and anonymous function values](02-function-values.md)
- [Closure environments](03-closures.md)
- [Consuming and stored closures](04-consuming-and-stored-closures.md)
- [Boxed callable environments](05-boxed-callables.md)
- [Recursion and tail calls](05-tail-calls.md)

[Generic constraints](../07-generics/index.md) allow one callback API to accept
different concrete callable representations without erasing their types.

# Iteration and generators

Iteration either consumes an owned iterable or lends access to a collection.
Comprehensions build fallible collection results. Generators own concrete
frames and resume only when advanced; construction and frame management add
no heap allocation.

- [Iterable values and borrowing](01-iterables.md)
- [Comprehensions and collection construction](02-comprehensions.md)
- [Generators](03-generators.md)
- [Enumeration and paired iteration](04-adapters.md)

[Loop control](../04-expressions/loops.md) defines binding scope, `break`,
`continue`, and return checking. [Collections](../../facilities/01-builtins/collections.md)
and [text](../../facilities/01-builtins/text.md) define their operations and ordering.

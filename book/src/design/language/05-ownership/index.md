# Ownership, borrowing, and lifetimes

An owner controls a value's lifetime. Copyable values can have independent
owners; affine values transfer their ownership when moved. References lend
access without taking ownership. Access permissions end at a loan's last use;
owned resources follow the specified destruction scopes.

- [Bindings, moves, and copies](01-bindings-moves-and-copies.md)
- [Borrowing and reborrowing](02-borrowing.md)
- [Reference binding reassignment](03-reference-rebinding.md)
- [Returned references](04-returned-references.md)
- [Optional and fallible references](05-optional-references.md)
- [Borrowed matching](06-borrowed-matching.md)
- [Deterministic destruction](07-destruction.md)

[Types](../03-types/index.md) define which values can be stored together.
[Iteration](../09-iteration/index.md) and
[resource scopes](../08-errors/03-resource-scopes.md) apply these same ownership
rules to their hidden owners.

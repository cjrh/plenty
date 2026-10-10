# Errors, allocation, and resource scopes

Recoverable failure is a value: `Option` represents absence and `Result`
represents success or an error. `?` propagates a failure through the enclosing
function with ordinary cleanup. Allocating operations expose checked failures;
fatal traps terminate without unwinding.

- [Standard sums and propagation](01-sums-and-propagation.md)
- [Allocation and traps](02-allocation-and-traps.md)
- [Resource scopes](03-resource-scopes.md)

[Destruction](../05-ownership/06-destruction.md) defines cleanup order.
[The entrypoint](../02-source/entrypoint.md) defines process exit status and
reporting of a `main` error.

# Errors, allocation, and resource scopes

Recoverable failure is a value: `Option` represents absence and `Result`
represents success or an error. `?` propagates a failure through the enclosing
function with ordinary cleanup. Allocating operations expose checked failures;
fatal traps terminate without unwinding.

There are no exceptions. A `try`, `except`, `raise`, or `finally` statement is
rejected with a hint toward `Result`:

```plenty-error
def main() -> ():
    try:
        pass
```
```error
`try` is not supported; Plenty has no exceptions: return `Result[T, E]`, propagate an error with `?`, and handle it with `match`
```

- [Standard sums and propagation](01-sums-and-propagation.md)
- [Allocation and traps](02-allocation-and-traps.md)
- [Resource scopes](03-resource-scopes.md)

[Destruction](../05-ownership/07-destruction.md) defines cleanup order.
[The entrypoint](../02-source/entrypoint.md) defines process exit status and
reporting of a `main` error. [`std.error`](../../stdlib/error.md) adds operation
and path context around a typed error and reports it on standard error.

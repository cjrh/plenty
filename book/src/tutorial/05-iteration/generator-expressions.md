# Pass a lazy expression to a consumer

A generator expression evaluates one element at a time. A consumer such as
`any` or `all` can stop before evaluating the rest:

```plenty
def qualifies(n: i64) -> bool:
    print("visit", n).unwrap()
    n == 2

def main() -> Result[(), Failure]:
    print(any(qualifies(n) for n in range(10)))?
    squares = (n * n for n in range(4) if n != 2)
    print(list[i64].from(squares)?)?
    Ok(())
```
```output
visit 0
visit 1
visit 2
True
[0, 1, 9]
```

The outer source evaluates when the generator is created. Filters, elements,
and inner `for` sources evaluate on demand. The generator owns its sources
and copies or moves any enclosing local values it uses; copied captures are
snapshots. Borrowed inputs are unsupported because a suspended frame cannot
retain a reference into its caller. Use `copy(values)?` to preserve an owned list.

Parentheses can be omitted for a sole argument. With additional arguments,
write `consume((n for n in source), other)`. Generator expressions follow the
same ownership, cleanup, and yield restrictions as generator functions;
they require no intermediate list or heap-allocated frame. A fallible operation
in their body must be handled explicitly, since `?` cannot propagate out of
a suspended generator.

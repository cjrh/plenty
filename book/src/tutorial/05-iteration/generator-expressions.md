# Pass a lazy expression to a consumer

A generator expression evaluates one element at a time, when its consumer
asks for the next one:

```plenty
def main() -> Result[(), AllocError | IoError]:
    squares = (n * n for n in range(4) if n != 2)
    print(list[i64].from(squares)?)?
    words = ["tea", "", "café"]?
    print(", ".join(w for w in words if w != "")?)?
    Ok(())
```
```output
[0, 1, 9]
tea, café
```

`separator.join` accepts any iterable of `str`, so the selected words are
joined without first collecting them into a list. A generator cannot be
measured in advance, so `join` grows a scratch buffer as pieces arrive; a list,
set, or dictionary is measured first and needs only the result's allocation.
A consumer that stops early, such as the [pipeline](pipelines.md) method
`any`, leaves the remaining elements unevaluated.

The outer source evaluates when the generator is created. Filters, elements,
and inner `for` sources evaluate on demand. The generator owns its sources
and copies or moves any enclosing local values it uses; copied captures are
snapshots. Borrowed inputs are unsupported because a suspended frame cannot
retain a reference into its caller. Use `copy(values)?` to preserve an owned list,
or consume a borrowing [pipeline](../10-functions/pipelines.md) locally instead.

Parentheses can be omitted for a sole argument. With additional arguments,
write `consume((n for n in source), other)`. Generator expressions follow the
same ownership, cleanup, and yield restrictions as generator functions;
they require no intermediate list or heap-allocated frame. A fallible operation
in their body must be handled explicitly, since `?` cannot propagate out of
a suspended generator.
